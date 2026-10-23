use super::*;

/// `GET /v1/agent/runs/{run_id}/tabs` — list live browser tabs.
pub async fn list_tabs(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
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
    let request_id = crate::request_id::current();
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
        // WS7.D: the tab's URL is held to the run's web policy before it is opened.
        crate::agent_policy_gate::gate_visit(&state, &handle.live, entry.zdr.is_active(), url)
            .map_err(|error| driver_err(&request_id, error))?;
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
    let request_id = crate::request_id::current();
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
    let request_id = crate::request_id::current();
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
