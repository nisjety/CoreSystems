use super::*;

/// The query of `GET .../tabs`. `grant_id` is how the attachment's delegate names its
/// attach grant (the lane check of `delegate_step` reads it from the step body; a GET has
/// none). The run's owner sends nothing.
#[derive(Debug, Default, Deserialize)]
pub struct TabListQuery {
    #[serde(default, alias = "grantId")]
    pub grant_id: Option<String>,
}

/// `GET /v1/agent/runs/{run_id}/tabs` — list live browser tabs.
///
/// The run's owner sees every tab. The live attachment's delegate (D2-074) sees only the
/// tab its attachment opened, the one it may act on: the person's own tabs, and anything
/// the ledger does not tie to this attachment, are not listed to it. Anyone else is the
/// same 404 as an unknown run.
pub async fn list_tabs(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<TabListQuery>,
) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let handle = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.clone()
        })
    };
    let Some(handle) = handle else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    let entry_arc = handle.entry.clone();

    let mut entry = entry_arc.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        // Not the owner: only the live attachment's delegate may list, and only its own tab.
        let Some(attach) = super::attach::delegate_attach_of(
            &handle,
            &entry,
            &claims,
            query.grant_id.as_deref(),
        ) else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        };
        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        let own = delegate_visible_tabs(&handle, &attach, tabs);
        return Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs: own })));
    }

    let tabs = state
        .agent_driver
        .list_tabs(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    prune_tab_openers(&mut entry, &tabs);

    Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
}

/// The tabs a delegate may see: the one its attachment opened, and only while the ledger
/// still ties that tab to this attachment. The ledger, not the browser, decides who holds
/// a tab, so a tab the person opened or took over under another id is never in the list.
pub(super) fn delegate_visible_tabs(
    handle: &RunHandle,
    attach: &AttachState,
    tabs: Vec<BrowserTab>,
) -> Vec<BrowserTab> {
    let ledger = handle.live.tabs();
    tabs.into_iter()
        .filter(|tab| {
            tab.tab_id == attach.tab_id
                && ledger
                    .get(&tab.tab_id)
                    .is_some_and(|meta| meta.attach_id.as_deref() == Some(attach.attach_id.as_str()))
        })
        .collect()
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

/// The tab a failed open kept (W2-U07): the id that a TIMEOUT names in `details.tab_id`, when
/// that id is a non-empty string. `None` for any other error. Only the id is read here; nothing
/// else from the error is copied anywhere.
pub(super) fn kept_tab_id(error: &QuarryError) -> Option<&str> {
    if error.code != ErrorCode::Timeout {
        return None;
    }
    error
        .details
        .as_ref()?
        .get("tab_id")?
        .as_str()
        .filter(|tab_id| !tab_id.is_empty())
}

/// `POST /v1/agent/runs/{run_id}/tabs` — open a new live browser tab.
pub async fn new_tab(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Json(body): Json<NewTabBody>,
) -> Result<Json<Envelope<BrowserTabData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // O5: the ownership check comes first and needs no lock (`owned_by`, the rule a step
    // uses), so a caller who does not own the run is refused before it counts as in flight.
    // Counting first let a stranger's call hold a run's expiry off for as long as it
    // queued on the run lock.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
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
    // REST/WebSocket steps receive. WS7.K: its snapshot is the grant layer of the tab's URL check.
    let grant_snapshot = if let Some(grant_id) = entry.grant_id.as_deref() {
        validate_run_grant(
            &state,
            &entry.org_id,
            grant_id,
            entry.key_id.as_deref(),
            &entry.constraints.allowed_domains,
            &entry.nav_policy,
        )
        .await
            .map_err(|error| driver_err(&request_id, error))?
    } else if state.require_browser_grants || entry.key_id.is_some() {
        return Err(status_err(
            StatusCode::FORBIDDEN,
            &request_id,
            "browser grant is required for tab creation",
        ));
    } else {
        crate::agent_policy_gate::GrantSnapshot::Absent
    };

    if let Some(url) = body.url.as_deref() {
        if !entry.nav_policy.allows(url, &entry.constraints.allowed_domains) {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser tab navigation is outside the run's allowed domains",
            ));
        }
        // WS7.D: the tab's URL is held to the run's web policy before it is opened, and (WS7.K)
        // to the grant snapshot and the caller's claim, which only tighten it.
        let step_claim = crate::agent_policy_gate::step_claim(&state.policy, &claims);
        crate::agent_policy_gate::gate_visit_for_step(
            &state,
            &handle.live,
            entry.zdr.is_active(),
            url,
            &crate::agent_policy_gate::StepLayers {
                grant: &grant_snapshot,
                claim: &step_claim,
            },
        )
        .map_err(|error| driver_err(&request_id, error))?;
        // WS8.B: run-aware, so an open-web run's new tab stays DNS-guarded even when the
        // engine allows private hosts.
        validate_navigation_for_run(
            &AgentAction::Navigate {
                url: url.to_owned(),
            },
            state.security.as_ref(),
            &entry.nav_policy,
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    }

    // Q3-E3.2-e1w: a person's origin extension is read again before the tab opens and, while
    // it stands, the tab starts from the widened boundary. A grant that is no longer usable is
    // dropped here (fail closed) and the tab runs under the run's own policy.
    super::origin_extension::revalidate_origin_extension(&state, &handle, &mut entry, None)
        .await
        .map_err(|error| driver_err(&request_id, error))?;

    // Tab creation bypasses ObservationRunner, so explicitly apply the
    // same run-scoped policy before this browser-owned navigation. The
    // Chromium Fetch listener then enforces it for redirects and every
    // request the new page initiates.
    state
        .agent_driver
        .configure_egress_policy(
            &entry.session,
            // WS7.G: the run's web-policy deny list is part of the boundary a new tab runs under.
            super::origin_extension::tab_egress_policy(&state, &handle, &entry),
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

    let tab = match state
        .agent_driver
        .new_tab(&entry.session, body.url.as_deref())
        .await
    {
        Ok(tab) => tab,
        Err(error) => {
            // W2-U07: a first navigation that timed out keeps its tab, and the tab is the run's.
            // It is remembered and recorded as a success records it, so the owner sees who opened
            // it, and the timeout is answered with its id so the caller can select or close it.
            // This bookkeeping is best effort: a failure in it is logged and never replaces the
            // timeout the caller gets.
            if let Some(kept) = kept_tab_id(&error) {
                match state.agent_driver.list_tabs(&entry.session).await {
                    Ok(tabs) => {
                        remember_tab_opener(
                            &mut entry,
                            kept,
                            TabOrigin {
                                initiated_by: admitted.initiator,
                                agent: admitted.agent.clone(),
                                unattributed: false,
                            },
                            &tabs,
                        );
                        if let Err(record_error) = record_browser_timeline_event(
                            &state,
                            &entry,
                            &handle.live,
                            BrowserTimelineEventKind::Tab {
                                operation: BrowserTabOperation::Opened,
                                initiated_by: Some(admitted.initiator),
                                agent: admitted.agent.clone(),
                                control_epoch: admitted.control_epoch,
                                tab_id: Some(kept.to_owned()),
                                active_tab_id: active_browser_tab_id(&tabs),
                            },
                        )
                        .await
                        {
                            tracing::warn!(
                                code = ?record_error.code,
                                "could not record a kept browser tab on the timeline"
                            );
                        }
                    }
                    Err(list_error) => {
                        tracing::warn!(
                            code = ?list_error.code,
                            "could not list the browser tabs after a timed-out open"
                        );
                    }
                }
            }
            return Err(driver_err(&request_id, error));
        }
    };
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
        &handle.live,
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
    let request_id = crate::request_id::current();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // O5: the ownership check comes first and needs no lock (`owned_by`, the rule a step
    // uses), so a caller who does not own the run is refused before it counts as in flight.
    // Counting first let a stranger's call hold a run's expiry off for as long as it
    // queued on the run lock.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
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
        &handle.live,
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
    let request_id = crate::request_id::current();
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(run_gone_err(&state, &claims, &request_id, &run_id).await);
    };

    // O5: the ownership check comes first and needs no lock (`owned_by`, the rule a step
    // uses), so a caller who does not own the run is refused before it counts as in flight.
    // Counting first let a stranger's call hold a run's expiry off for as long as it
    // queued on the run lock.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
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
        &handle.live,
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
    let request_id = crate::request_id::current();
    // WS9.C: validate the optional filters before any lookup. The cursor is the
    // caller's sequence; the filter scans the driver window and keeps `limit` matches.
    let filter = DevtoolsFilter::from_query(&query)
        .map_err(|message| status_err(StatusCode::BAD_REQUEST, &request_id, message))?;
    let after_sequence = query.after_sequence.unwrap_or(0);
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            (handle.entry.clone(), handle.live.clone())
        })
    };
    let Some((entry_arc, live)) = entry_arc else {
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

    let scanned = state
        .agent_driver
        .devtools_events(&entry.session, after_sequence, DEVTOOLS_SCAN_LIMIT)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    let (events, cursor) = filter_devtools_events(&scanned, &filter);
    let next_sequence = cursor.unwrap_or(after_sequence);
    if let Some(last) = events.last() {
        record_browser_timeline_event(
            &state,
            &entry,
            &live,
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
            next_sequence,
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
    let request_id = crate::request_id::current();
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

/// Events one filtered devtools read may examine: the driver's whole buffer.
const DEVTOOLS_SCAN_LIMIT: usize = 512;
/// Longest substring filter, in characters.
const DEVTOOLS_FILTER_MAX_CHARS: usize = 200;
/// Most console levels one filter may name.
const DEVTOOLS_LEVELS_MAX: usize = 5;

/// A validated devtools read (WS9.C): the constraints a caller asked for and
/// how many matches to return. Built only by [`DevtoolsFilter::from_query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DevtoolsFilter {
    /// Canonical levels (`debug`, `info`, `warning`, `error`). Empty: every level.
    levels: Vec<&'static str>,
    /// Lower-cased substring. Matched against `text`, `url` and `name` only.
    substring: Option<String>,
    category: Option<&'static str>,
    tab_id: Option<String>,
    limit: usize,
}

impl DevtoolsFilter {
    /// Validate the query. Errors are fixed messages: a caller's raw value is
    /// never echoed back.
    pub(super) fn from_query(query: &DevtoolsQuery) -> Result<Self, &'static str> {
        let limit = query.limit.unwrap_or(100).clamp(1, 512);

        let mut levels: Vec<&'static str> = Vec::new();
        if let Some(raw) = query.levels.as_deref() {
            let names: Vec<&str> = raw
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect();
            if names.len() > DEVTOOLS_LEVELS_MAX {
                return Err("too many levels in the devtools filter");
            }
            for name in names {
                let canonical =
                    canonical_level(name).ok_or("unsupported level in the devtools filter")?;
                if !levels.contains(&canonical) {
                    levels.push(canonical);
                }
            }
        }

        let raw_filter = query.filter.as_deref().unwrap_or("");
        if raw_filter.chars().any(char::is_control) {
            return Err("devtools filter contains a control character");
        }
        let text = raw_filter.trim();
        let substring = if text.is_empty() {
            None
        } else if text.chars().count() > DEVTOOLS_FILTER_MAX_CHARS {
            return Err("devtools filter is too long");
        } else {
            Some(text.to_lowercase())
        };

        let category = match query.category.as_deref().map(|value| value.trim().to_ascii_lowercase()) {
            None => None,
            Some(value) if value.is_empty() => None,
            Some(value) => Some(match value.as_str() {
                "console" => "console",
                "network" => "network",
                "lifecycle" => "lifecycle",
                _ => return Err("unsupported devtools category"),
            }),
        };

        let tab_id = match query.tab_id.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(tab)
                if tab.len() <= 64
                    && tab
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') =>
            {
                Some(tab.to_owned())
            }
            Some(_) => return Err("invalid tab id in the devtools filter"),
        };

        Ok(Self {
            levels,
            substring,
            category,
            tab_id,
            limit,
        })
    }
}

/// Canonical console level for a caller's level name. `warn`, `log` and
/// `verbose` are aliases of `warning`, `info` and `debug`; the same mapping
/// normalises the level a driver reported.
fn canonical_level(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "debug" | "verbose" => Some("debug"),
        "info" | "log" => Some("info"),
        "warning" | "warn" => Some("warning"),
        "error" => Some("error"),
        _ => None,
    }
}

/// Pure filter over one scanned window (WS9.C). Returns at most `filter.limit`
/// matching events in sequence order, and the sequence of the last event this
/// call examined (`None` for an empty window). A caller resumes from that
/// sequence, so a window with no match still advances the cursor.
pub(super) fn filter_devtools_events(
    events: &[BrowserDevtoolsEvent],
    filter: &DevtoolsFilter,
) -> (Vec<BrowserDevtoolsEvent>, Option<u64>) {
    let mut matched = Vec::new();
    let mut cursor = None;
    for event in events {
        cursor = Some(event.sequence);
        if devtools_event_matches(event, filter) {
            matched.push(event.clone());
            if matched.len() >= filter.limit {
                break;
            }
        }
    }
    (matched, cursor)
}

fn devtools_event_matches(event: &BrowserDevtoolsEvent, filter: &DevtoolsFilter) -> bool {
    if let Some(category) = filter.category {
        if !event.category.eq_ignore_ascii_case(category) {
            return false;
        }
    }
    if let Some(tab_id) = filter.tab_id.as_deref() {
        if event.tab_id.as_deref() != Some(tab_id) {
            return false;
        }
    }
    if !filter.levels.is_empty() {
        let level = event.level.as_deref().and_then(canonical_level);
        if !level.is_some_and(|level| filter.levels.contains(&level)) {
            return false;
        }
    }
    if let Some(needle) = filter.substring.as_deref() {
        let hit = [event.text.as_deref(), event.url.as_deref(), Some(event.name.as_str())]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(needle));
        if !hit {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// W3 E3.2-e2: consent for attach-mode tabs (the route half; the ledger half is in
// `tab_ledger.rs`).
//
// Attach mode (WS8.G) offers the person's visible tab to an agent attachment. Before that tab
// can be attached, its current origin must carry the person's consent: an origin-extension
// grant that passes the E3.2-e1 gate (`agent_policy_gate::origin_extension_from_grant` and
// `check_origin_extension`: org caps, then deny beats approval) and whose hosts cover the tab's
// host, which must itself pass the run's web policy. A policy deny is recorded on the tab and is
// sticky; an approval is recorded with the host it was checked for. The attach itself
// (`tab_ledger::attach_consented_tab`) then refuses any tab without an approval for the host it
// is on, so a denied tab is never attached.
//
// Flag: follows `QUARRY_EDGE__ORIGIN_CONSENT` (default off). Off, every check is refused with 403
// `origin_consent_disabled` before any driver or broker call. Dark: no route calls
// `admit_attach_mode_tab` until the Model Plane attach driver's attach-mode request lands.
// ---------------------------------------------------------------------------

/// What a consent check approved for an attach-mode tab. Ids and closed labels only: no host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) struct AttachTabConsent {
    pub tab_id: String,
    /// The origin-extension grant that approved the tab's origin.
    pub grant_id: String,
    pub scope: crate::agent_policy_gate::ConsentScope,
    /// The tab's epoch when the consent was recorded (unchanged by the check).
    pub tab_epoch: u64,
}

/// The host of a tab's page, lower case and without a trailing dot, for an http(s) page only.
/// `None` for anything else (a blank tab, a `file:` or `chrome:` page, no URL).
pub(super) fn tab_consent_host(url: Option<&str>) -> Option<String> {
    let parsed = Url::parse(url?).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether an origin extension covers `host`: one of its hosts, or a host under one of them on
/// a dot boundary (the allow-list rule the widened egress boundary applies).
pub(super) fn extension_covers_host(
    extension: &crate::agent_policy_gate::OriginExtension,
    host: &str,
) -> bool {
    extension.all_hosts().iter().any(|entry| {
        host == entry
            || host
                .strip_suffix(entry.as_str())
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// Whether a refusal of the E3.2-e1 gate is a deny that sticks to the tab. A malformed grant, a
/// grant of another run or a host the extension does not list is no deny of the tab: nothing is
/// approved either, so the tab still cannot be attached, and a correct grant may follow.
pub(super) fn consent_refusal_is_sticky(
    refusal: crate::agent_policy_gate::OriginExtensionRefusal,
) -> bool {
    use crate::agent_policy_gate::OriginExtensionRefusal;
    matches!(
        refusal,
        OriginExtensionRefusal::Denied
            | OriginExtensionRefusal::OrgDisabled
            | OriginExtensionRefusal::OrgScope
    )
}

/// The typed error of a ledger consent refusal. The details carry closed labels only.
pub(super) fn tab_consent_error(error: TabConsentError) -> QuarryError {
    let (code, message) = match error {
        TabConsentError::NotFound => (ErrorCode::NotFound, "the tab is not open on this run"),
        TabConsentError::Denied { .. } => (
            ErrorCode::SecurityBlocked,
            "consent for this tab's origin was denied; the tab is never attached",
        ),
        TabConsentError::ConsentRequired => (
            ErrorCode::Forbidden,
            "this tab's origin has no consent for attach mode",
        ),
        TabConsentError::NotHumanHeld => (
            ErrorCode::Conflict,
            "only a tab the person holds can be attached",
        ),
        TabConsentError::AlreadyAttached => (
            ErrorCode::Conflict,
            "the tab already belongs to an attachment",
        ),
        TabConsentError::NoAttachedAgent => (
            ErrorCode::Conflict,
            "no live attachment is pinned to this tab",
        ),
        TabConsentError::Stale { .. } => (
            ErrorCode::Conflict,
            "the tab's epoch has moved since it was read",
        ),
    };
    let mut details = serde_json::json!({ "reason": error.as_str() });
    match error {
        TabConsentError::Denied { reason } => details["denial"] = serde_json::json!(reason),
        TabConsentError::Stale { current } => details["tab_epoch"] = serde_json::json!(current),
        _ => {}
    }
    QuarryError::new(code, message).with_details(details)
}

fn count_attach_tab_consent_refused(run_id: &str, reason: &'static str) {
    metrics::counter!("quarry_attach_tab_consent_refused_total", "reason" => reason).increment(1);
    // Closed labels only: never a host or a URL.
    tracing::warn!(
        target: "quarry_edge::agent",
        run_id = %run_id,
        reason,
        "attach-mode tab consent refused"
    );
}

/// Check and record the person's consent for attaching their visible tab `tab_id` (attach mode),
/// reading the flag from the process environment.
#[allow(dead_code)]
pub(super) async fn admit_attach_mode_tab(
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    tab_id: &str,
    extension_grant_id: &str,
) -> QuarryResult<AttachTabConsent> {
    admit_attach_mode_tab_with(
        crate::agent_policy_gate::origin_consent_enabled(),
        state,
        handle,
        entry,
        tab_id,
        extension_grant_id,
    )
    .await
}

/// [`admit_attach_mode_tab`] with the flag passed in (tests). The caller holds the run's entry
/// lock. Order: the flag, the run (live, not a key run, not ZDR, has a grant), the tab from the
/// driver (open, the active tab, held by the person), a sticky deny (before any broker call), the
/// tab's origin, the run grant as gate 4 checks it, the origin-extension grant (E3.2-e1 reading),
/// org caps and deny-beats-approval (E3.2-e1 gate), the tab host's own web-policy check, coverage
/// of the tab host by the extension; then the approval is recorded.
#[allow(dead_code)]
pub(super) async fn admit_attach_mode_tab_with(
    enabled: bool,
    state: &AppState,
    handle: &RunHandle,
    entry: &RunEntry,
    tab_id: &str,
    extension_grant_id: &str,
) -> QuarryResult<AttachTabConsent> {
    use crate::agent_policy_gate::{
        check_origin_extension, origin_extension_from_grant, ExtensionRun, OriginExtensionRefusal,
    };
    let run_id = entry.run_id.to_string();
    let ledger_refusal = |error: TabConsentError| {
        count_attach_tab_consent_refused(&run_id, error.as_str());
        tab_consent_error(error)
    };
    let gate_refusal = |refusal: OriginExtensionRefusal| {
        count_attach_tab_consent_refused(&run_id, refusal.as_str());
        refusal.to_error()
    };

    if !enabled {
        return Err(gate_refusal(OriginExtensionRefusal::FlagOff));
    }
    if entry.closed {
        return Err(QuarryError::new(ErrorCode::NotFound, "agent run is not live")
            .with_details(serde_json::json!({ "reason": "run_not_live" })));
    }
    // An API key has no person to consent (INNH-10).
    if entry.key_id.is_some() {
        return Err(gate_refusal(OriginExtensionRefusal::ApiKeyRun));
    }
    // Attach refuses ZDR runs (WS8.F); attach mode does too.
    if entry.zdr.is_active() {
        count_attach_tab_consent_refused(&run_id, "attach_zdr_refused");
        return Err(QuarryError::new(ErrorCode::Forbidden, "attach_zdr_refused")
            .with_details(serde_json::json!({ "reason": "attach_zdr_refused" })));
    }
    let Some(run_grant_id) = entry.grant_id.as_deref() else {
        return Err(gate_refusal(OriginExtensionRefusal::GrantInvalid));
    };

    // The tab as the browser has it now. Tabs the ledger does not know are the person's.
    let tabs = state.agent_driver.list_tabs(&entry.session).await?;
    {
        let ids: Vec<&str> = tabs.iter().map(|tab| tab.tab_id.as_str()).collect();
        let mut ledger = handle.live.tabs();
        ledger.retain_live(&ids);
        for tab in &tabs {
            ledger.seed(&tab.tab_id, OpenedBy::Human);
        }
    }
    let Some(tab) = tabs.iter().find(|tab| tab.tab_id == tab_id) else {
        return Err(ledger_refusal(TabConsentError::NotFound));
    };
    let (prior, holder) = {
        let ledger = handle.live.tabs();
        (
            ledger.consent(tab_id).cloned(),
            ledger.get(tab_id).map(|meta| meta.holder),
        )
    };
    // A deny sticks to the tab: no broker call can lift it.
    if let Some(TabConsent::Denied { reason }) = prior {
        return Err(ledger_refusal(TabConsentError::Denied { reason }));
    }
    if holder != Some(BrowserTabHolder::Human) {
        return Err(ledger_refusal(TabConsentError::NotHumanHeld));
    }
    // Attach mode is for the tab the person is looking at.
    if !tab.active {
        count_attach_tab_consent_refused(&run_id, "attach_tab_not_visible");
        return Err(QuarryError::new(
            ErrorCode::Conflict,
            "attach mode takes the person's visible tab only",
        )
        .with_details(serde_json::json!({ "reason": "attach_tab_not_visible" })));
    }
    // A blank or non-web page has no origin to consent to; not a deny (it may navigate).
    let Some(host) = tab_consent_host(tab.url.as_deref()) else {
        return Err(ledger_refusal(TabConsentError::ConsentRequired));
    };

    // The run grant first, exactly as gate 4 checks it, then its snapshot for the org caps and
    // the grant layer.
    let grant_snapshot = validate_run_grant(
        state,
        &entry.org_id,
        run_grant_id,
        entry.key_id.as_deref(),
        &entry.constraints.allowed_domains,
        &entry.nav_policy,
    )
    .await?;
    let run_grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, run_grant_id)
        .await?;
    let extension_grant = state
        .grant_validator
        .validate_for_org(&entry.org_id, extension_grant_id)
        .await?;
    let extension = origin_extension_from_grant(
        &extension_grant,
        &ExtensionRun {
            parent_grant_id: run_grant_id,
            owner_id: &entry.actor_id,
            zdr: false,
        },
    )
    .map_err(&gate_refusal)?;

    let deny = |reason: OriginExtensionRefusal| {
        if consent_refusal_is_sticky(reason) {
            // The tab is open (seeded above), so this records; a race with a close is harmless.
            let _ = handle.live.tabs().deny_consent(tab_id, reason.as_str());
        }
        gate_refusal(reason)
    };
    let run_denied: &[String] = match &entry.nav_policy {
        super::human_browse::RunNavPolicy::OpenWeb { denied } => denied,
        super::human_browse::RunNavPolicy::Allowlist => &[],
    };
    check_origin_extension(state, &handle.live, false, run_denied, &run_grant, &extension)
        .map_err(&deny)?;
    // The tab's own host, which may sit under a listed host, must pass the web policy too, with
    // the run grant's snapshot as the grant layer (WS7.K). No caller token reaches this check,
    // so it states no claim layer.
    let no_claim = quarry_core::web_policy::OrgInput::Absent;
    let consent_layers = crate::agent_policy_gate::StepLayers {
        grant: &grant_snapshot,
        claim: &no_claim,
    };
    if crate::agent_policy_gate::gate_visit_for_step(
        state,
        &handle.live,
        false,
        &format!("https://{host}/"),
        &consent_layers,
    )
    .is_err()
        || run_denied.iter().any(|denied_host| {
            host == *denied_host
                || host
                    .strip_suffix(denied_host.as_str())
                    .is_some_and(|rest| rest.ends_with('.'))
        })
    {
        return Err(deny(OriginExtensionRefusal::Denied));
    }
    if !extension_covers_host(&extension, &host) {
        return Err(ledger_refusal(TabConsentError::ConsentRequired));
    }

    let tab_epoch = {
        let mut ledger = handle.live.tabs();
        ledger
            .approve_consent(tab_id, &host, &extension.grant_id)
            .map_err(&ledger_refusal)?;
        ledger.get(tab_id).map_or(0, |meta| meta.epoch)
    };
    metrics::counter!("quarry_attach_tab_consent_approved_total", "scope" => extension.scope.as_str())
        .increment(1);
    tracing::info!(
        target: "quarry_edge::agent",
        run_id = %run_id,
        scope = extension.scope.as_str(),
        "attach-mode tab consent recorded"
    );
    Ok(AttachTabConsent {
        tab_id: tab_id.to_owned(),
        grant_id: extension.grant_id,
        scope: extension.scope,
        tab_epoch,
    })
}

#[cfg(test)]
mod attach_tab_consent_tests {
    use super::*;
    use crate::agent_policy_gate::{ConsentScope, OriginExtension, OriginExtensionRefusal};

    fn extension(start: &str, hosts: &[&str]) -> OriginExtension {
        OriginExtension {
            grant_id: "grant_ext_1".to_owned(),
            start_host: start.to_owned(),
            hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
            scope: ConsentScope::Conversation,
        }
    }

    #[test]
    fn the_tab_host_is_read_from_web_pages_only() {
        assert_eq!(
            tab_consent_host(Some("https://Shop.Example.com/cart?x=1")),
            Some("shop.example.com".to_owned())
        );
        assert_eq!(
            tab_consent_host(Some("http://shop.example.com./")),
            Some("shop.example.com".to_owned())
        );
        assert_eq!(tab_consent_host(Some("about:blank")), None);
        assert_eq!(tab_consent_host(Some("file:///etc/passwd")), None);
        assert_eq!(tab_consent_host(Some("chrome://settings")), None);
        assert_eq!(tab_consent_host(Some("not a url")), None);
        assert_eq!(tab_consent_host(None), None);
    }

    #[test]
    fn an_extension_covers_its_hosts_and_their_subdomains_only() {
        let ext = extension("shop.example.com", &["login.example.net"]);
        assert!(extension_covers_host(&ext, "shop.example.com"));
        assert!(extension_covers_host(&ext, "login.example.net"));
        assert!(extension_covers_host(&ext, "eu.shop.example.com"));
        assert!(!extension_covers_host(&ext, "example.com"), "a parent is not covered");
        assert!(!extension_covers_host(&ext, "evilshop.example.com"), "a dot boundary only");
        assert!(!extension_covers_host(&ext, "other.example.org"));
    }

    #[test]
    fn only_policy_denies_stick_to_the_tab() {
        assert!(consent_refusal_is_sticky(OriginExtensionRefusal::Denied));
        assert!(consent_refusal_is_sticky(OriginExtensionRefusal::OrgDisabled));
        assert!(consent_refusal_is_sticky(OriginExtensionRefusal::OrgScope));
        for loose in [
            OriginExtensionRefusal::FlagOff,
            OriginExtensionRefusal::GrantInvalid,
            OriginExtensionRefusal::HostInvalid,
            OriginExtensionRefusal::TooManyHosts,
            OriginExtensionRefusal::ZdrScope,
            OriginExtensionRefusal::OrgUnreadable,
            OriginExtensionRefusal::ApiKeyRun,
        ] {
            assert!(!consent_refusal_is_sticky(loose), "{}", loose.as_str());
        }
    }

    #[test]
    fn ledger_refusals_map_to_typed_errors_with_closed_labels() {
        let denied = tab_consent_error(TabConsentError::Denied {
            reason: "origin_extension_denied",
        });
        assert_eq!(denied.code, ErrorCode::SecurityBlocked);
        let required = tab_consent_error(TabConsentError::ConsentRequired);
        assert_eq!(required.code.http_status(), 403);
        assert_eq!(
            tab_consent_error(TabConsentError::NotFound).code.http_status(),
            404
        );
        for conflict in [
            TabConsentError::NotHumanHeld,
            TabConsentError::AlreadyAttached,
            TabConsentError::NoAttachedAgent,
            TabConsentError::Stale { current: 3 },
        ] {
            assert_eq!(tab_consent_error(conflict).code.http_status(), 409);
        }
    }

    #[test]
    fn the_flag_off_refusal_is_403_before_anything_else() {
        // `admit_attach_mode_tab_with(false, ..)` returns this before any driver or broker call.
        let error = OriginExtensionRefusal::FlagOff.to_error();
        assert_eq!(error.code.http_status(), 403);
    }
}

#[cfg(test)]
mod devtools_filter_tests {
    use super::*;

    fn query() -> DevtoolsQuery {
        DevtoolsQuery {
            after_sequence: None,
            limit: None,
            levels: None,
            filter: None,
            category: None,
            tab_id: None,
        }
    }

    fn event(
        sequence: u64,
        level: Option<&str>,
        text: Option<&str>,
        url: Option<&str>,
    ) -> BrowserDevtoolsEvent {
        BrowserDevtoolsEvent {
            sequence,
            tab_id: Some("tab-1".to_owned()),
            category: "console".to_owned(),
            name: "Runtime.consoleAPICalled".to_owned(),
            level: level.map(str::to_owned),
            method: None,
            url: url.map(str::to_owned),
            status: None,
            text: text.map(str::to_owned),
            timestamp_ms: sequence,
            payload: serde_json::json!({ "args": "payload-only" }),
        }
    }

    fn sequences(events: &[BrowserDevtoolsEvent]) -> Vec<u64> {
        events.iter().map(|event| event.sequence).collect()
    }

    #[test]
    fn devtools_filter_levels_normalise_warn_and_log() {
        let events = vec![
            event(1, Some("warning"), Some("a"), None),
            event(2, Some("info"), Some("b"), None),
            event(3, Some("log"), Some("c"), None),
            event(4, Some("error"), Some("d"), None),
            event(5, Some("debug"), Some("e"), None),
        ];
        let run = |levels: &str| {
            let mut q = query();
            q.levels = Some(levels.to_owned());
            let filter = DevtoolsFilter::from_query(&q).expect("valid levels");
            sequences(&filter_devtools_events(&events, &filter).0)
        };
        assert_eq!(run("warn"), vec![1], "warn is the warning level");
        assert_eq!(run("log"), vec![2, 3], "log is the info level");
        assert_eq!(run("verbose"), vec![5], "verbose is the debug level");
        assert_eq!(run("warn,error"), vec![1, 4]);

        let mut q = query();
        q.levels = Some("trace".to_owned());
        assert!(DevtoolsFilter::from_query(&q).is_err(), "an unknown level is refused");
        let mut q = query();
        q.levels = Some("debug,info,log,warn,warning,error".to_owned());
        assert!(DevtoolsFilter::from_query(&q).is_err(), "more than five levels is refused");
    }

    #[test]
    fn devtools_filter_substring_is_case_insensitive_and_ignores_payload() {
        let mut payload_only = event(4, Some("info"), None, None);
        payload_only.payload = serde_json::json!({ "args": "BOOM" });
        let events = vec![
            event(1, Some("error"), Some("Boom happened"), None),
            event(2, Some("info"), None, Some("https://x.test/BOOM")),
            event(3, Some("info"), Some("quiet"), None),
            payload_only,
        ];
        let mut q = query();
        q.filter = Some("boom".to_owned());
        let filter = DevtoolsFilter::from_query(&q).expect("valid filter");
        assert_eq!(
            sequences(&filter_devtools_events(&events, &filter).0),
            vec![1, 2],
            "text and url match in any case; payload never matches"
        );
    }

    #[test]
    fn devtools_filter_resumes_from_scanned_sequence() {
        let events: Vec<BrowserDevtoolsEvent> = (1..=6)
            .map(|sequence| {
                let level = if sequence % 2 == 0 { "error" } else { "info" };
                event(sequence, Some(level), Some("row"), None)
            })
            .collect();
        let mut q = query();
        q.levels = Some("error".to_owned());
        q.limit = Some(1);
        let filter = DevtoolsFilter::from_query(&q).expect("valid filter");

        let (first, cursor) = filter_devtools_events(&events, &filter);
        assert_eq!(sequences(&first), vec![2], "the first error row");
        assert_eq!(cursor, Some(2), "the cursor stops at the matched row");

        let rest: Vec<BrowserDevtoolsEvent> = events
            .iter()
            .filter(|event| event.sequence > cursor.unwrap_or(0))
            .cloned()
            .collect();
        let (second, next) = filter_devtools_events(&rest, &filter);
        assert_eq!(sequences(&second), vec![4], "resuming continues after the cursor");
        assert_eq!(next, Some(4));

        let quiet: Vec<BrowserDevtoolsEvent> = events
            .iter()
            .filter(|event| event.sequence < 3 && event.sequence % 2 == 1)
            .cloned()
            .collect();
        let (none, scanned_to) = filter_devtools_events(&quiet, &filter);
        assert!(none.is_empty());
        assert_eq!(scanned_to, Some(1), "a window with no match still advances the cursor");
    }

    #[test]
    fn devtools_filter_rejects_overlong_filter() {
        let mut q = query();
        q.filter = Some("a".repeat(201));
        assert!(DevtoolsFilter::from_query(&q).is_err(), "201 characters is refused");

        let mut q = query();
        q.filter = Some("a".repeat(200));
        assert!(DevtoolsFilter::from_query(&q).is_ok(), "200 characters is allowed");

        let mut q = query();
        q.filter = Some("bad\nvalue".to_owned());
        assert!(DevtoolsFilter::from_query(&q).is_err(), "a control character is refused");

        let mut q = query();
        q.category = Some("cookies".to_owned());
        assert!(DevtoolsFilter::from_query(&q).is_err(), "an unknown category is refused");

        let mut q = query();
        q.tab_id = Some("tab/../x".to_owned());
        assert!(DevtoolsFilter::from_query(&q).is_err(), "a malformed tab id is refused");
    }
}
