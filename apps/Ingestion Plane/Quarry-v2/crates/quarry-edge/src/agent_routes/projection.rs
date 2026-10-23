use super::*;

pub(super) fn safe_display_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return String::new();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

pub(super) fn browser_session_tab(tab: BrowserTab, opened_by: Option<TabOrigin>) -> BrowserSessionTab {
    BrowserSessionTab {
        opened_by,
        tab_id: tab.tab_id,
        title: tab.title,
        url: tab
            .url
            .as_deref()
            .map(safe_display_url)
            .filter(|value| !value.is_empty()),
        active: tab.active,
    }
}

pub(super) fn active_browser_tab_id(tabs: &[BrowserTab]) -> Option<String> {
    tabs.iter()
        .find(|tab| tab.active)
        .map(|tab| tab.tab_id.clone())
}

pub(super) fn live_browser_session_projection(
    entry: &RunEntry,
    tabs: Vec<BrowserTab>,
) -> BrowserSessionProjectionData {
    BrowserSessionProjectionData {
        run_id: entry.run_id.to_string(),
        lease_id: entry.lease.lease_id.to_string(),
        profile_id: entry.lease.profile_id.to_string(),
        profile_storage: if entry.lease.persist_profile {
            "persistent"
        } else {
            "ephemeral"
        },
        profile_scope: entry.profile_scope,
        viewport: entry.lease.viewport,
        current_url: safe_display_url(&entry.ctx.current_url),
        step: entry.ctx.step,
        status: AgentRunStatus::Active,
        live: true,
        zdr: entry.zdr.is_active(),
        control_mode: entry.control_mode,
        control_epoch: entry.control_epoch,
        execution_tier: entry.execution_tier,
        tabs: tabs
            .into_iter()
            .map(|tab| {
                let opened_by = entry.tab_origins.get(&tab.tab_id).cloned();
                browser_session_tab(tab, opened_by)
            })
            .collect(),
        last_observation: entry.last_observation.clone(),
        grant_id: entry.grant_id.clone(),
    }
}

pub(super) fn checkpoint_browser_session_projection(
    checkpoint: AgentRunCheckpoint,
) -> BrowserSessionProjectionData {
    BrowserSessionProjectionData {
        run_id: checkpoint.run_id,
        lease_id: checkpoint.lease_id,
        profile_id: checkpoint.profile_id,
        profile_storage: if checkpoint.persist_profile {
            "persistent"
        } else {
            "ephemeral"
        },
        profile_scope: checkpoint.profile_scope,
        viewport: checkpoint.viewport,
        current_url: safe_display_url(&checkpoint.current_url),
        step: checkpoint.step,
        status: checkpoint.status,
        live: false,
        zdr: checkpoint.zdr.is_active(),
        control_mode: checkpoint.control_mode,
        control_epoch: checkpoint.control_epoch,
        // Ephemeral evidence runs are ZDR and never checkpointed.
        execution_tier: BrowserExecutionTier::Chromium,
        tabs: Vec::new(),
        last_observation: None,
        grant_id: checkpoint.grant_id,
    }
}

pub(super) fn browser_timeline_action(receipt: StepReceipt) -> BrowserTimelineItem {
    let (outcome, error_code) = match receipt.outcome {
        StepOutcome::Completed { .. } => ("completed", None),
        StepOutcome::Failed { error_code, .. } => ("failed", Some(error_code)),
        StepOutcome::Skipped { .. } => ("skipped", None),
    };
    BrowserTimelineItem {
        id: receipt.receipt_id,
        occurred_at: receipt.finished_at.to_rfc3339(),
        detail: BrowserTimelineItemDetail::Action {
            action: browser_action_name(&receipt.action),
            outcome,
            error_code,
            initiated_by: receipt.initiated_by,
            agent: receipt.agent,
            control_epoch: receipt.control_epoch,
        },
    }
}

pub(super) fn browser_timeline_event(event: BrowserTimelineEvent) -> BrowserTimelineItem {
    let detail = match event.event {
        BrowserTimelineEventKind::Control {
            mode,
            initiated_by,
            control_epoch,
        } => BrowserTimelineItemDetail::Control {
            mode,
            initiated_by,
            control_epoch,
        },
        BrowserTimelineEventKind::Tab {
            operation,
            tab_id,
            active_tab_id,
            initiated_by,
            agent,
            control_epoch,
        } => BrowserTimelineItemDetail::Tab {
            operation,
            tab_id,
            active_tab_id,
            initiated_by,
            agent,
            control_epoch,
        },
        BrowserTimelineEventKind::Devtools {
            event_count,
            last_sequence,
        } => BrowserTimelineItemDetail::Devtools {
            event_count,
            last_sequence,
        },
        BrowserTimelineEventKind::Lifecycle { state, initiated_by } => {
            BrowserTimelineItemDetail::Lifecycle { state, initiated_by }
        }
    };
    BrowserTimelineItem {
        id: event.event_id,
        occurred_at: event.occurred_at.to_rfc3339(),
        detail,
    }
}

pub(super) fn timeline_sort_key(item: &BrowserTimelineItem) -> (&str, &str) {
    (&item.occurred_at, &item.id)
}

/// `GET /v1/agent/runs/{run_id}/browser-session` — the browser-session
/// projection owned by Quarry. Live Chromium state is read from the run;
/// after a restart only the non-ZDR checkpoint projection is available.
/// Both paths require the verified tenant and the signed initiating actor.
pub async fn browser_session(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<BrowserSessionProjectionData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
        status_err(StatusCode::BAD_REQUEST, &request_id, "invalid agent run id")
    })?;

    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    if let Some(entry_arc) = entry_arc {
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
            .map_err(|error| driver_err(&request_id, error))?;
        prune_tab_openers(&mut entry, &tabs);
        return Ok(Json(Envelope::ok(
            request_id,
            live_browser_session_projection(&entry, tabs),
        )));
    }

    let checkpoint = state
        .receipts
        .load_run_checkpoint(&claims.org_id, &parsed_run_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let Some(checkpoint) = checkpoint else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    if !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }

    Ok(Json(Envelope::ok(
        request_id,
        checkpoint_browser_session_projection(checkpoint),
    )))
}

/// `GET /v1/agent/runs/{run_id}/browser-session/timeline` — a bounded,
/// cursor-paginated owner timeline. It combines actor-bound action
/// receipts with Quarry's compact control/tab/devtools/lifecycle audit
/// records; neither stream contains raw browser contents.
pub async fn browser_session_timeline(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<BrowserTimelineQuery>,
) -> Result<Json<Envelope<BrowserTimelineData>>, ApiErr> {
    let request_id = crate::request_id::current();
    let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
        status_err(StatusCode::BAD_REQUEST, &request_id, "invalid agent run id")
    })?;
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).map(|handle| {
            if handle.live.owned_by(&claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    let (org_id, actor_id, zdr) = if let Some(entry_arc) = entry_arc {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        (entry.org_id.clone(), entry.actor_id.clone(), entry.zdr)
    } else {
        let checkpoint = state
            .receipts
            .load_run_checkpoint(&claims.org_id, &parsed_run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        let Some(checkpoint) = checkpoint else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        };
        if !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        (checkpoint.org_id, checkpoint.actor_id, checkpoint.zdr)
    };
    if zdr.is_active() {
        return Err(status_err(
            StatusCode::CONFLICT,
            &request_id,
            "browser timeline is unavailable for a Zero Data Retention run",
        ));
    }

    let receipts = state
        .receipts
        .list_for_actor(&org_id, &actor_id, &parsed_run_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let events = state
        .receipts
        .list_browser_timeline_events(&org_id, &actor_id, &parsed_run_id)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    let mut items = receipts
        .into_iter()
        .map(browser_timeline_action)
        .chain(events.into_iter().map(browser_timeline_event))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| timeline_sort_key(left).cmp(&timeline_sort_key(right)));

    let start = if let Some(cursor) = query.cursor.as_deref() {
        let Some(index) = items.iter().position(|item| item.id == cursor) else {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "browser timeline cursor is invalid",
            ));
        };
        index.saturating_add(1)
    } else {
        0
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let page = items
        .into_iter()
        .skip(start)
        .take(limit.saturating_add(1))
        .collect::<Vec<_>>();
    let has_more = page.len() > limit;
    let mut page = page;
    if has_more {
        let _ = page.pop();
    }
    let next_cursor = has_more.then(|| {
        page.last()
            .expect("non-empty browser timeline page when has_more")
            .id
            .clone()
    });

    Ok(Json(Envelope::ok(
        request_id,
        BrowserTimelineData {
            items: page,
            next_cursor,
        },
    )))
}

#[cfg(test)]
mod grant_projection_tests {
    use super::*;

    fn checkpoint(grant_id: Option<&str>) -> AgentRunCheckpoint {
        serde_json::from_value(serde_json::json!({
            "org_id": "org-a",
            "actor_id": "user-a",
            "run_id": "run_01",
            "profile_id": "prof_01",
            "step": 1,
            "current_url": "https://example.com/a?q=secret",
            "constraints": { "max_steps": 12 },
            "zdr": "off",
            "grant_id": grant_id,
        }))
        .expect("checkpoint fixture")
    }

    #[test]
    fn the_owner_projection_carries_the_session_grant_id_as_grant_id() {
        let projection = checkpoint_browser_session_projection(checkpoint(Some("grant-abc")));
        let json = serde_json::to_value(&projection).expect("projection serializes");
        assert_eq!(json["grantId"], "grant-abc");
    }

    #[test]
    fn a_run_without_a_grant_omits_grant_id() {
        let projection = checkpoint_browser_session_projection(checkpoint(None));
        let json = serde_json::to_value(&projection).expect("projection serializes");
        assert!(json.get("grantId").is_none(), "{json}");
    }
}
