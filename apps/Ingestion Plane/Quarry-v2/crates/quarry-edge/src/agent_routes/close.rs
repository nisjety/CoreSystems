//! Ending a run: the owner's `DELETE`, the one path both the owner and the sweeper end a run
//! through (`finish_run`), and the shutdown drain. Split from `lifecycle.rs` (which starts runs)
//! to keep both files near the 650-line target (WS2.B).

use super::*;

/// `DELETE /v1/agent/runs/{run_id}` — release the leased session.
pub async fn close_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<serde_json::Value>>, ApiErr> {
    let request_id = crate::request_id::current();

    let Some(handle) = lookup_run(&state, &run_id) else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    // Ownership first, before touching shared state.
    if !handle.live.owned_by(&claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }

    let usage = finish_run(&state, &run_id, handle, CloseReason::Explicit)
        .await
        .map_err(|error| driver_err(&request_id, error))?;
    // WS6.B: an agent run reports what it spent; a person's session has no budget to report.
    let mut body = serde_json::json!({ "closed": true });
    if let Some(usage) = usage {
        body["usage"] = usage;
    }
    Ok(Json(Envelope::ok(request_id, body)))
}

/// Why a run ended.
pub(super) enum CloseReason {
    /// Its owner closed it.
    Explicit,
    /// The sweeper released it.
    Expired(ExpireVerdict),
    /// WS6.J: the edge is shutting down. The wait for a running action and the usage and
    /// release steps are each bounded by this duration, so one hung run cannot hold up the
    /// exit. Like an expiry it leaves a non-ZDR checkpoint `active` (the run can be resumed
    /// after the restart), and it emits `agent.expired` with `reason: "shutdown"`.
    Drain(std::time::Duration),
}

/// The error `finish_run` returns for a drain when an action still holds the run.
const DRAIN_IN_USE: &str = "drain_in_use";
/// The error `finish_run` returns for a drain when the browser session did not release.
const DRAIN_RELEASE_FAILED: &str = "drain_release_failed";

fn drain_error(reason: &'static str) -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        "a run could not be released by the shutdown drain",
    )
    .with_details(serde_json::json!({ "reason": reason }))
}

/// WS8.H: how long a close waits for an action that still runs on a lane.
const LANE_WAIT: Duration = Duration::from_secs(5);

/// WS8.H: every lane of a run, the person's and the live attachment's. Read under the entry lock.
pub(super) fn lane_refs(entry: &RunEntry, live: &RunLive) -> Vec<LaneRef> {
    let mut lanes = vec![entry.human_lane.clone()];
    if let Some(attach) = live.attaches().live() {
        lanes.push(attach.lane.clone());
    }
    lanes
}

/// WS8.H: wait, bounded in total, for each lane's running action to finish, so the browser
/// session is not released under a call that is still running. A lane still held when the
/// wait ends is logged and the session is released anyway (the drain has always done so).
pub(super) async fn wait_for_lanes(run_id: &str, lanes: &[LaneRef], wait: Duration) {
    let deadline = tokio::time::Instant::now() + wait;
    for lane in lanes {
        if tokio::time::timeout_at(deadline, lane.0.lock()).await.is_err() {
            tracing::warn!(run_id, "close: an action still holds a lane; releasing anyway");
            return;
        }
    }
}

/// The one path that ends a run, for the owner's `DELETE` and for the sweeper. It
/// writes the durable record first (a failure leaves the run registered and
/// untouched), then marks the run closed so an action that was waiting for the lock
/// is refused, takes it out of the registry and always releases the browser session,
/// even while another request still holds a reference to the run.
///
/// An explicit close also closes the checkpoint; an expiry leaves it `active`, so a
/// non-ZDR run can be resumed. A ZDR run leaves nothing durable either way.
pub(super) async fn finish_run(
    state: &AppState,
    run_id: &str,
    handle: RunHandle,
    reason: CloseReason,
) -> QuarryResult<Option<serde_json::Value>> {
    // Waits for an action that is still running (a drain waits only so long).
    let mut entry = match reason {
        CloseReason::Drain(wait) => match tokio::time::timeout(wait, handle.entry.lock()).await {
            Ok(entry) => entry,
            Err(_) => {
                tracing::warn!(run_id, "drain: an action still holds this run");
                return Err(drain_error(DRAIN_IN_USE));
            }
        },
        _ => handle.entry.lock().await,
    };
    if entry.closed {
        return Ok(None);
    }
    let zdr = entry.zdr;
    let rid = entry.run_id.clone();
    if !zdr.is_active() {
        let lifecycle = match reason {
            CloseReason::Explicit => BrowserTimelineLifecycle::Closed,
            CloseReason::Expired(_) | CloseReason::Drain(_) => BrowserTimelineLifecycle::Expired,
        };
        let appended = state
            .receipts
            .append_browser_timeline_event(BrowserTimelineEvent::new(
                entry.org_id.clone(),
                entry.actor_id.clone(),
                rid.to_string(),
                BrowserTimelineEventKind::Lifecycle {
                    state: lifecycle,
                    // The sweeper (or the shutdown) ended the run, not a person or an agent.
                    initiated_by: matches!(reason, CloseReason::Expired(_) | CloseReason::Drain(_))
                        .then_some(BrowserTimelineInitiator::System),
                },
            ))
            .await;
        if let Err(error) = appended {
            if matches!(reason, CloseReason::Drain(_)) {
                // The browser must still be released at shutdown; the checkpoint stays active.
                tracing::warn!(
                    run_id,
                    error_class = crate::zdr_log::class(&error),
                    "drain: the timeline record failed; releasing anyway"
                );
            } else {
                return Err(error);
            }
        }
        if matches!(reason, CloseReason::Explicit) {
            state
                .receipts
                .close_run_checkpoint(&entry.org_id, &rid)
                .await?;
        }
    }

    entry.closed = true;
    state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .remove(run_id);
    // WS4.J: the ZDR tombstone, sent only by the path that won the removal (the
    // `closed` check above returns first for a racing second caller). Off unless the
    // flag is on; fail open; ids, class and counts only.
    {
        let org_id = entry.org_id.clone();
        let run_id_text = rid.to_string();
        let mut run_end = crate::zdr_tombstone::RunEnd::new(
            &run_id_text,
            &org_id,
            crate::zdr_tombstone::Kind::AgentRun,
        )
        .ended_as(match reason {
            CloseReason::Explicit => crate::zdr_tombstone::Outcome::Ok,
            CloseReason::Expired(_) => crate::zdr_tombstone::Outcome::Timeout,
            CloseReason::Drain(_) => crate::zdr_tombstone::Outcome::Cancelled,
        });
        if let Some(budget) = entry.budget.as_ref() {
            run_end = run_end.steps(budget.ledger.steps);
        }
        crate::zdr_tombstone::emit_run_end(state.tombstones.as_ref(), zdr.is_active(), &run_end);
    }
    // WS6.K: the run-duration histogram, recorded by the one path that won the removal
    // above (a racing second caller returned at the `closed` check).
    record_run_duration(&handle.live, now_ms());
    // WS6.B: the usage as it stood when the run ended (absent for a person's session).
    let usage = entry
        .budget
        .as_ref()
        .map(|budget| budget.usage(std::time::Instant::now()));
    // WS6.C-e: this segment's usage, emitted once the entry is released below.
    let segment_usage = entry
        .budget
        .as_ref()
        .map(|budget| budget.segment_usage(std::time::Instant::now()));
    let usage_org = entry.org_id.clone();
    let usage_actor = entry.actor_id.clone();
    let session = entry.session.clone();
    // WS8.H: the lanes are named while the entry is held, and waited for once it is released.
    let lanes = lane_refs(&entry, &handle.live);
    drop(entry);
    wait_for_lanes(run_id, &lanes, LANE_WAIT).await;
    if let Some(segment) = segment_usage.as_ref() {
        let outcome = match reason {
            CloseReason::Explicit => crate::usage_attrs::Outcome::Ok,
            CloseReason::Expired(_) | CloseReason::Drain(_) => crate::usage_attrs::Outcome::Expired,
        };
        let rid_text = rid.to_string();
        let emitted = crate::usage_attrs::emit_run_segment(
            &*state.usage,
            &rid_text,
            &usage_org,
            &usage_actor,
            zdr.is_active(),
            segment,
            outcome,
        );
        match reason {
            CloseReason::Drain(wait) => {
                if tokio::time::timeout(wait, emitted).await.is_err() {
                    tracing::warn!(run_id, "drain: the usage segment was not accepted in time");
                }
            }
            _ => emitted.await,
        }
    }
    let released = match reason {
        CloseReason::Drain(wait) => {
            match tokio::time::timeout(wait, state.agent_driver.release(session)).await {
                Ok(released) => released,
                Err(_) => Err(QuarryError::new(
                    ErrorCode::Timeout,
                    "releasing the browser session timed out",
                )),
            }
        }
        _ => state.agent_driver.release(session).await,
    };
    let release_failed = released.is_err();
    if let Err(error) = released {
        tracing::warn!(run_id, error_class = crate::zdr_log::class(&error), "releasing a run's browser session failed");
    }

    match reason {
        CloseReason::Explicit => {
            state
                .event_sink
                .emit_for_zdr(
                    zdr,
                    rid.clone(),
                    EventType::AgentCompleted,
                    serde_json::json!({}),
                    format!("{rid}:agent_completed"),
                )
                .await;
        }
        CloseReason::Expired(verdict) => {
            state
                .event_sink
                .emit_for_zdr(
                    zdr,
                    rid.clone(),
                    EventType::AgentExpired,
                    serde_json::json!({
                        "reason": verdict.reason.as_str(),
                        "idle_s": verdict.idle_s,
                        "age_s": verdict.age_s,
                    }),
                    format!("{rid}:agent_expired"),
                )
                .await;
        }
        CloseReason::Drain(_) => {
            // WS6.J: the viewer is told why the run ended before its stream closes.
            let (idle_s, age_s) = handle.live.idle_and_age_s(now_ms());
            state
                .event_sink
                .emit_for_zdr(
                    zdr,
                    rid.clone(),
                    EventType::AgentExpired,
                    serde_json::json!({
                        "reason": "shutdown",
                        "idle_s": idle_s,
                        "age_s": age_s,
                    }),
                    format!("{rid}:agent_expired"),
                )
                .await;
        }
    }
    // WS10.S3: after the terminal event above, the run's trace (replay ring and live
    // channel) is released, so a live subscriber sees the channel close.
    state.event_sink.close_run(&rid);
    if release_failed && matches!(reason, CloseReason::Drain(_)) {
        return Err(drain_error(DRAIN_RELEASE_FAILED));
    }
    Ok(usage)
}

/// Release every live browser session. Runs concurrently (8 at a time) with
/// `per_run_timeout` on the wait for a running action and on each release, so one
/// hung Chromium cannot hold up the rest. A non-ZDR run's checkpoint is left `Active`, so it can be resumed
/// with `resume_run_id` after the restart; nothing is written for a ZDR run.
pub async fn drain_runs(
    runs: &AgentRuns,
    driver: &Arc<dyn quarry_browser::BrowserDriver>,
    per_run_timeout: std::time::Duration,
) -> DrainReport {
    use futures_util::StreamExt;
    let snapshot: Vec<(String, RunHandle)> = runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .drain()
        .collect();
    let outcomes: Vec<Option<bool>> = futures_util::stream::iter(snapshot)
        .map(|(run_id, handle)| {
            let driver = driver.clone();
            async move {
                // A request that still holds a clone of the run does not keep its session:
                // wait (bounded) for an action that is running, mark the run closed so a
                // request queued behind it is refused instead of running on a released
                // session, and release a clone of the session, as `finish_run` does.
                let Ok(mut entry) = tokio::time::timeout(per_run_timeout, handle.entry.lock()).await
                else {
                    tracing::warn!(%run_id, "drain: an action still holds this run");
                    return None;
                };
                if entry.closed {
                    // `finish_run` got there first and releases the session itself.
                    return Some(true);
                }
                entry.closed = true;
                let session = entry.session.clone();
                let lanes = lane_refs(&entry, &handle.live);
                drop(entry);
                wait_for_lanes(&run_id.to_string(), &lanes, per_run_timeout).await;
                match tokio::time::timeout(per_run_timeout, driver.release(session)).await {
                    Ok(Ok(())) => Some(true),
                    Ok(Err(error)) => {
                        tracing::warn!(%run_id, error_class = crate::zdr_log::class(&error), "drain: release failed");
                        Some(false)
                    }
                    Err(_) => {
                        tracing::warn!(%run_id, "drain: release timed out");
                        Some(false)
                    }
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    let mut report = DrainReport::default();
    for outcome in outcomes {
        match outcome {
            Some(true) => report.released += 1,
            Some(false) => report.failed += 1,
            None => report.in_use += 1,
        }
    }
    // WS6.I: the runs this drain released ended with the `agent.expired` reason `shutdown`.
    crate::metrics::record_run_expired("shutdown", report.released as u64);
    report
}

/// WS6.J: the shutdown drain. Ends every live run through `finish_run` (the one path that
/// ends a run), so each one writes its last usage segment, emits `agent.expired` with
/// `reason: "shutdown"` and releases its browser session exactly once. Runs concurrently
/// (8 at a time); `per_run_timeout` bounds the wait for a running action, the usage step and
/// the release of each run, so one hung Chromium cannot hold up the rest.
///
/// A non-ZDR run's checkpoint is left `active` (the run can be resumed with `resume_run_id`
/// after the restart). A ZDR run writes no timeline record and has no checkpoint. A run that
/// another path (an explicit close or the sweeper) is already ending is not ended twice.
/// Calling it again is a no-op once the registry is empty.
pub async fn drain_runs_for_shutdown(
    state: &AppState,
    per_run_timeout: std::time::Duration,
) -> DrainReport {
    use futures_util::StreamExt;
    let snapshot = state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .snapshot();
    let outcomes: Vec<Option<bool>> = futures_util::stream::iter(snapshot)
        .map(move |(run_id, handle)| async move {
            match finish_run(state, &run_id, handle, CloseReason::Drain(per_run_timeout)).await {
                Ok(_) => Some(true),
                Err(error) => {
                    let reason = error
                        .details
                        .as_ref()
                        .and_then(|details| details["reason"].as_str());
                    if reason == Some(DRAIN_IN_USE) {
                        None
                    } else {
                        Some(false)
                    }
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    let mut report = DrainReport::default();
    for outcome in outcomes {
        match outcome {
            Some(true) => report.released += 1,
            Some(false) => report.failed += 1,
            None => report.in_use += 1,
        }
    }
    crate::metrics::record_run_expired("shutdown", report.released as u64);
    report
}
