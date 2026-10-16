use super::*;

/// Decide whether a typed event should reach the App Shell. The
/// agent loop today emits only `agent.*` / `action.*` /
/// `observation.ready` events; the explicit allow-list keeps
/// future internal/telemetry events out of the user-facing
/// stream without changing the wire contract.
pub(super) fn is_user_facing_event(event_type: quarry_core::event::EventType) -> bool {
    use quarry_core::event::EventType;
    matches!(
        event_type,
        EventType::AgentStarted
            | EventType::ActionStarted
            | EventType::ActionCompleted
            | EventType::ActionFailed
            | EventType::ObservationReady
            | EventType::AgentCompleted
            | EventType::AgentFailed
    )
}

/// Map a typed event to its SSE event name. The App Shell
/// dispatches on this string.
pub(super) fn event_name(event_type: quarry_core::event::EventType) -> &'static str {
    use quarry_core::event::EventType;
    match event_type {
        EventType::AgentStarted => "agent.started",
        EventType::ActionStarted => "action.started",
        EventType::ActionCompleted => "action.completed",
        EventType::ActionFailed => "action.failed",
        EventType::ObservationReady => "observation.ready",
        EventType::AgentCompleted => "agent.completed",
        EventType::AgentFailed => "agent.failed",
        // Defensive: every other event type is filtered before
        // this is called, but never panic on a future type.
        _ => "internal",
    }
}

pub(super) fn normalize_viewport(
    viewport: Option<BrowserViewport>,
) -> Result<Option<BrowserViewport>, String> {
    let Some(viewport) = viewport else {
        return Ok(None);
    };
    if !(320..=3840).contains(&viewport.width) || !(240..=2160).contains(&viewport.height) {
        return Err("viewport must be between 320x240 and 3840x2160".to_owned());
    }
    let device_scale_factor = if viewport.device_scale_factor > 0.0 {
        viewport.device_scale_factor.clamp(0.5, 4.0)
    } else {
        1.0
    };
    Ok(Some(BrowserViewport {
        device_scale_factor,
        ..viewport
    }))
}

pub(super) fn default_stream_viewport() -> BrowserViewport {
    BrowserViewport {
        width: 1280,
        height: 800,
        device_scale_factor: 1.0,
        is_mobile: false,
    }
}

pub(super) fn live_frame_format(value: Option<&str>) -> Result<LiveFrameFormat, &'static str> {
    match value.map(str::trim).unwrap_or("jpeg") {
        "jpeg" | "jpg" => Ok(LiveFrameFormat::Jpeg),
        "png" => Ok(LiveFrameFormat::Png),
        _ => Err("format must be jpeg or png"),
    }
}

pub(super) fn live_frame_options(
    query: &FrameQuery,
    viewport: BrowserViewport,
) -> Result<LiveFrameOptions, &'static str> {
    Ok(LiveFrameOptions {
        format: live_frame_format(query.format.as_deref())?,
        quality: query.quality.unwrap_or(65).clamp(1, 100),
        max_width: query
            .max_width
            .unwrap_or(viewport.width)
            .clamp(320, viewport.width.max(320)),
        max_height: query
            .max_height
            .unwrap_or(viewport.height)
            .clamp(240, viewport.height.max(240)),
        every_nth_frame: query.every_nth_frame.unwrap_or(1).clamp(1, 10),
        timeout_ms: query.timeout_ms.unwrap_or(1_000).clamp(100, 5_000),
    })
}

pub(super) fn stream_interval(query: &FrameStreamQuery) -> Duration {
    Duration::from_millis(query.interval_ms.unwrap_or(250).clamp(75, 2_000))
}

/// `GET /v1/agent/runs/{run_id}/frame` — return one transient live frame.
///
/// This never writes an artifact. It is for live preview/human takeover only;
/// durable evidence still flows through ObservationRunner screenshots.
pub async fn live_frame(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<FrameQuery>,
) -> Result<Json<Envelope<LiveFrameData>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    if !state.agent_driver.capabilities().full_visual_fidelity {
        return Err(driver_err(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot produce governed full-fidelity live frames",
            ),
        ));
    }
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
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

    let viewport = entry.lease.viewport.unwrap_or_else(default_stream_viewport);
    let options = live_frame_options(&query, viewport)
        .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;

    let frame = state
        .agent_driver
        .live_frame(&entry.session, options)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    Ok(Json(Envelope::ok(
        request_id,
        LiveFrameData {
            mime_type: frame.mime_type,
            data_base64: frame.data_base64,
            zdr: matches!(entry.zdr, ZdrMode::On),
        },
    )))
}

/// `GET /v1/agent/runs/{run_id}/live-view` — single-shot live-view
/// reference. W2 — the App Shell calls this on connect and
/// again whenever the `live_view` SSE event's `expires_at`
/// elapses, so the iframe URL rotates without polling the
/// frame stream.
///
/// Returns 200 with the `LiveViewRef` (or `null` when the driver
/// has no live-view URL — the local Chromiumoxide fallback).
/// Returns 404 when the run is unknown or not owned by the
/// caller. The success shape is the same `LiveViewRef` the SSE
/// `live_view` event emits, byte-for-byte.
pub async fn live_view(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<Option<quarry_core::driver_meta::LiveViewRef>>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
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
    let lv = state
        .agent_driver
        .live_view(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, lv)))
}

/// `GET /v1/agent/runs/{run_id}/events/stream` — W3 — typed
/// agent-event SSE. Sends the same `quarry_core::event::Event`
/// envelope the `EventSink` already broadcasts, in the order the
/// agent loop emitted them, until the run closes. The App Shell
/// uses this to render the action timeline next to the frame
/// stream.
///
/// Filtering: `internal.*` and `telemetry.*` event types are
/// dropped. The agent loop only emits a small set of
/// human-facing event types (`agent.*`, `action.*`,
/// `observation.ready`) so the filter is currently a no-op,
/// but it documents the wire contract for future telemetry
/// events that the agent loop may add without surfacing.
///
/// Reconnect: `?from_seq=N` replays from the durable event
/// history first (Postgres `quarry_event_history` when
/// configured, no-op otherwise), then streams the live tail.
pub async fn agent_events_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<AgentEventsQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    let parsed_run: RunKind =
        run_id
            .parse()
            .map_err(|e: quarry_core::error::QuarryError| {
                status_err(StatusCode::BAD_REQUEST, &request_id, &e.message)
            })?;
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
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
    drop(entry);

    // Replay the durable tail (best-effort: if the postgres
    // history backend is not configured, the replay emits no
    // events and the live tail still starts immediately).
    // NOTE: postgres-queue JobHistoryEvent replay is wired
    // separately via `GET /v1/runs/:id/events?after_seq=`; the
    // agent live tail here is intentionally live-only so we
    // don't conflate JobHistoryEvent<->Event types during the
    // fleet promotion. `?from_seq=` remains accepted for
    // forward-compat but is currently a no-op live tail.
    let mut history_rx: Option<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = quarry_core::event::Event> + Send>>,
    > = None;
    let _ = &query.from_seq;

    let mut live_rx = state.event_sink.subscribe(&parsed_run);
    let max_events = query.max_events.unwrap_or(0);
    let mut emitted: u64 = 0;

    let stream = async_stream::stream! {
        // Drain the replay tail first (if any). The replay
        // stream yields events already in seq order.
        if let Some(replay) = history_rx.as_mut() {
            use futures_util::StreamExt;
            while let Some(evt) = replay.next().await {
                if is_user_facing_event(evt.event_type) {
                    if max_events > 0 && emitted >= max_events {
                        yield Ok(Event::default().event("done").data("max_events"));
                        return;
                    }
                    emitted += 1;
                    if let Ok(event) = Event::default()
                        .event(event_name(evt.event_type))
                        .id(evt.seq.to_string())
                        .json_data(&evt)
                    {
                        yield Ok(event);
                    }
                }
            }
        }
        // Then stream the live tail.
        loop {
            if max_events > 0 && emitted >= max_events {
                yield Ok(Event::default().event("done").data("max_events"));
                break;
            }
            match live_rx.recv().await {
                Ok(evt) => {
                    if !is_user_facing_event(evt.event_type) {
                        continue;
                    }
                    emitted += 1;
                    let name = event_name(evt.event_type);
                    let evt_for_log = evt.clone();
                    match Event::default()
                        .event(name)
                        .id(evt.seq.to_string())
                        .json_data(&evt)
                    {
                        Ok(event) => yield Ok(event),
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                ?evt_for_log,
                                "agent events stream: failed to serialize event; skipping"
                            );
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // Slow consumer: tell the App Shell to
                    // reconnect with `?from_seq=N` to backfill
                    // the gap. We keep streaming from the next
                    // available event so a real-time-only
                    // consumer still sees fresh data.
                    yield Ok(Event::default()
                        .event("lagged")
                        .data(format!("{{\"skipped\":{skipped}}}")));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    // Run closed. The terminal `agent.completed` /
                    // `agent.failed` event is already in the
                    // history; emit `done` so the App Shell
                    // closes its EventSource.
                    yield Ok(Event::default().event("done").data("run_closed"));
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `GET /v1/agent/runs/{run_id}/frames/stream` — stream transient live frames.
///
/// Frame events are never persisted. This is the read-only visual channel
/// for live preview; browser actions still flow through `/step`.
pub async fn live_frame_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<FrameStreamQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let request_id = RequestKind::new().to_string();
    if !state.agent_driver.capabilities().full_visual_fidelity {
        return Err(driver_err(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot produce governed full-fidelity live frames",
            ),
        ));
    }
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(&run_id).cloned()
    };
    let Some(entry_arc) = entry_arc else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };

    let (session, viewport, zdr) = {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        (
            entry.session.clone(),
            entry.lease.viewport.unwrap_or_else(default_stream_viewport),
            entry.zdr,
        )
    };
    let options = live_frame_options(&query.frame, viewport)
        .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;
    let interval = stream_interval(&query);
    let max_frames = query.max_frames.unwrap_or(0).min(7_200);
    let driver = state.agent_driver.clone();
    let zdr_active = matches!(zdr, ZdrMode::On);

    let stream = async_stream::stream! {
        let mut sequence = 0_u64;
        // W2 — emit the live-view reference as the first event so
        // the App Shell can decide whether to render the cloud
        // session (Browserbase/Browserless/Kernel) or fall back
        // to the local frame stream (Chromiumoxide). Cloud
        // providers return a URL with an `expires_at`; the
        // App Shell triggers a `/v1/agent/runs/{id}/live-view`
        // refresh when that deadline elapses.
        if let Ok(Some(ref lv)) = driver.live_view(&session).await {
            match Event::default().event("live_view").json_data(lv) {
                Ok(event) => yield Ok(event),
                Err(_) => {
                    // Live-view is best-effort; missing it must
                    // not stop the frame stream. The App Shell
                    // just falls back to the screenshot frames.
                }
            }
        }
        loop {
            if max_frames > 0 && sequence >= max_frames {
                yield Ok(Event::default().event("done").data("max_frames"));
                break;
            }

            match driver.live_frame(&session, options).await {
                Ok(frame) => {
                    sequence = sequence.saturating_add(1);
                    let payload = LiveFrameStreamData {
                        sequence,
                        mime_type: frame.mime_type,
                        data_base64: frame.data_base64,
                        zdr: zdr_active,
                    };
                    match Event::default()
                        .event("frame")
                        .id(sequence.to_string())
                        .json_data(&payload)
                    {
                        Ok(event) => yield Ok(event),
                        Err(err) => {
                            yield Ok(Event::default().event("error").data(err.to_string()));
                            break;
                        }
                    }
                }
                Err(err) => {
                    yield Ok(Event::default().event("error").data(err.to_string()));
                    break;
                }
            }

            tokio::time::sleep(interval).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
