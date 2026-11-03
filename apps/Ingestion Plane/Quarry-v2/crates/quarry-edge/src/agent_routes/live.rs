use super::*;

/// Decide whether a typed event should reach the App Shell. The
/// agent loop today emits only `agent.*` / `action.*` /
/// `observation.ready` events; the explicit allow-list keeps
/// future internal/telemetry events out of the user-facing
/// stream without changing the wire contract.
///
/// X-12: `agent.budget_exhausted` is user-facing (once per segment, when `on`
/// refuses); its payload is the refusal's figures only, no URL and no page.
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
            | EventType::AgentExpired
            | EventType::AgentBudgetExhausted
            | EventType::AgentFailed
    )
}

/// WS6.J: the next event of a run's channel, or `None` when shutdown has asked the streams
/// to end (so graceful shutdown never waits on a viewer).
///
/// Biased toward the channel: the drain emits each run's `agent.expired` and only then closes
/// the streams, so when both are ready at the stream's next poll the buffered event must go
/// out first. With the default random branch order a viewer could be sent `done` without the
/// event that says why the run ended.
pub(super) async fn recv_unless_closing<T: Clone>(
    rx: &mut tokio::sync::broadcast::Receiver<T>,
) -> Option<Result<T, tokio::sync::broadcast::error::RecvError>> {
    tokio::select! {
        biased;
        received = rx.recv() => Some(received),
        () = crate::drain::streams_closing() => None,
    }
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
        EventType::AgentExpired => "agent.expired",
        EventType::AgentBudgetExhausted => "agent.budget_exhausted",
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
    let request_id = crate::request_id::current();
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
    let lv = state
        .agent_driver
        .live_view(&entry.session)
        .await
        .map_err(|e| driver_err(&request_id, e))?;
    Ok(Json(Envelope::ok(request_id, lv)))
}

/// `GET /v1/agent/runs/{run_id}/events/stream` — typed agent-event SSE. Sends the
/// `quarry_core::event::Event` envelope in the order the run emitted it (the `seq`
/// of the run's trace), until the run closes. The App Shell uses this to render the
/// action timeline next to the frame stream.
///
/// Filtering: only the user-facing types in `is_user_facing_event` are sent. Every
/// other event still advances the connection's `seq`, so ids stay meaningful.
///
/// Resume (WS10.S3): `?from_seq=N`, else the `Last-Event-ID` header a reconnecting
/// EventSource sends (`from_seq` wins when both are present). The stream replays the
/// run's in-memory trace ring after that seq, then tails the live channel with no
/// gap and no duplicate. When the ring no longer holds the events the client missed,
/// the stream first sends `replay_gap` with `{requested, oldest_retained,
/// fallback}`; the client then reads the durable browser-session timeline for the
/// older part. The ring is bounded per run (`QUARRY_EDGE__TRACE_RING_EVENTS`,
/// `QUARRY_EDGE__TRACE_RING_MAX_BYTES`) and lives only as long as the run, so a
/// reconnect after the run closed ends at once with `done` (`run_closed`).
///
/// A slow consumer that falls behind the broadcast buffer gets `lagged`, and the
/// stream re-snapshots from the ring after the last seq it sent.
pub async fn agent_events_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<AgentEventsQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let request_id = crate::request_id::current();
    let parsed_run: RunKind =
        run_id
            .parse()
            .map_err(|e: quarry_core::error::QuarryError| {
                status_err(StatusCode::BAD_REQUEST, &request_id, &e.message)
            })?;
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };
    let entry = handle.entry.lock().await;
    if !caller_owns_live_run(&entry, &claims) {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    }
    drop(entry);

    // The resume point: `?from_seq=` wins over the `Last-Event-ID` header.
    let resume_from = query.from_seq.or_else(|| last_event_id(&headers));
    // A person watching the run keeps it from expiring idle (not from the backstop).
    let viewer = handle.live.viewer_guard();
    let subscription = match resume_from {
        Some(from_seq) => state.event_sink.subscribe_with_replay(&parsed_run, from_seq),
        None => None,
    };
    // A run without a trace has no ring to replay; it gets the live tail only.
    //
    // D2-015: a run whose channel is already gone is a closed run. The plain `subscribe`
    // would create a channel for it that nothing ever closes, and this stream would wait
    // on it for good; `subscribe_open` answers `None` for it, and the stream ends with
    // `done` (`run_closed`) like a reconnect after the close does.
    let (replay, gap, rx, start_seen) = match subscription {
        Some(sub) => (sub.replay, sub.gap, Some(sub.rx), resume_from),
        None => (Vec::new(), None, state.event_sink.subscribe_open(&parsed_run), None),
    };
    let max_events = query.max_events.unwrap_or(0);
    let sink = state.event_sink.clone();
    // INNH-10: a key's token lives at most two minutes, and every call needs a live one, so a
    // stream a key opened ends when the token that opened it expires (`done: token_expired`).
    // The verifier's clock leeway does not extend it. The client reconnects with a fresh token.
    let token_deadline = claims.is_api_key().then(|| {
        let remaining = u64::try_from(claims.exp.saturating_sub(chrono::Utc::now().timestamp()))
            .unwrap_or(0);
        tokio::time::Instant::now() + Duration::from_secs(remaining)
    });

    let stream = async_stream::stream! {
        let _viewer = viewer;
        let Some(mut rx) = rx else {
            yield Ok(Event::default().event("done").data("run_closed"));
            return;
        };
        let mut emitted: u64 = 0;
        let mut last_seen: Option<u64> = start_seen;
        let mut pending: std::collections::VecDeque<quarry_core::event::Event> = replay.into();
        if let Some(gap) = gap {
            yield Ok(replay_gap_event(gap));
        }
        loop {
            if max_events > 0 && emitted >= max_events {
                yield Ok(Event::default().event("done").data("max_events"));
                break;
            }
            if token_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                yield Ok(Event::default().event("done").data("token_expired"));
                break;
            }
            let evt = match pending.pop_front() {
                Some(evt) => evt,
                None => match {
                    match token_deadline {
                        Some(deadline) => {
                            match tokio::time::timeout_at(deadline, recv_unless_closing(&mut rx)).await {
                                Ok(received) => received,
                                Err(_elapsed) => {
                                    yield Ok(Event::default().event("done").data("token_expired"));
                                    break;
                                }
                            }
                        }
                        None => recv_unless_closing(&mut rx).await,
                    }
                } {
                    None => {
                        // WS6.J: the edge is shutting down; the run's own `agent.expired`
                        // (reason `shutdown`) was sent before this.
                        yield Ok(Event::default().event("done").data("shutdown"));
                        break;
                    }
                    Some(Ok(evt)) => evt,
                    Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped))) => {
                        // Slow consumer: say so, then re-snapshot from the ring after the
                        // last seq this connection sent, so the live tail resumes without a gap.
                        yield Ok(Event::default()
                            .event("lagged")
                            .data(format!("{{\"skipped\":{skipped}}}")));
                        match sink.subscribe_with_replay(&parsed_run, last_seen.unwrap_or(0)) {
                            Some(resumed) => {
                                rx = resumed.rx;
                                pending = resumed.replay.into();
                                if let Some(gap) = resumed.gap {
                                    yield Ok(replay_gap_event(gap));
                                }
                            }
                            None => {
                                yield Ok(Event::default().event("done").data("run_closed"));
                                break;
                            }
                        }
                        continue;
                    }
                    Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                        // Run closed. The terminal `agent.completed` / `agent.failed` event
                        // is already sent; `done` tells the App Shell to close its EventSource.
                        yield Ok(Event::default().event("done").data("run_closed"));
                        break;
                    }
                },
            };
            // A replayed event the connection already sent (or a repeat from the live tail).
            if last_seen.is_some_and(|seen| evt.seq <= seen) {
                continue;
            }
            last_seen = Some(evt.seq);
            if !is_user_facing_event(evt.event_type) {
                continue;
            }
            emitted += 1;
            let name = event_name(evt.event_type);
            match Event::default()
                .event(name)
                .id(evt.seq.to_string())
                .json_data(&evt)
            {
                Ok(event) => yield Ok(event),
                Err(_) => {
                    // The event's payload can hold the page's URL, title and text, so the
                    // line names its type and sequence number, never the event itself.
                    tracing::warn!(
                        error_class = "serialize",
                        event_type = ?evt.event_type,
                        seq = evt.seq,
                        "agent events stream: failed to serialize event; skipping"
                    );
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// The `Last-Event-ID` a reconnecting EventSource sends: this stream's SSE `id:` is the
/// run's `seq`, so the header is the resume point.
fn last_event_id(headers: &axum::http::HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The `replay_gap` event: the resume point is older than the ring. The fallback names
/// the durable read that covers the missed part.
fn replay_gap_event(gap: quarry_runtime::ReplayGap) -> Event {
    Event::default().event("replay_gap").data(
        serde_json::json!({
            "requested": gap.requested,
            "oldest_retained": gap.oldest_retained,
            "fallback": "browser-session/timeline",
        })
        .to_string(),
    )
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
    let request_id = crate::request_id::current();
    if !state.agent_driver.capabilities().full_visual_fidelity {
        return Err(driver_err(
            &request_id,
            QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot produce governed full-fidelity live frames",
            ),
        ));
    }
    let handle = lookup_run(&state, &run_id);
    let Some(handle) = handle else {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            &request_id,
            "agent run not found",
        ));
    };

    let (session, viewport, zdr) = {
        let entry = handle.entry.lock().await;
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

    let viewer = handle.live.viewer_guard();
    let stream = async_stream::stream! {
        let _viewer = viewer;
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
            if crate::drain::streams_are_closing() {
                yield Ok(Event::default().event("done").data("shutdown"));
                break;
            }
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

            tokio::select! {
                () = tokio::time::sleep(interval) => {}
                () = crate::drain::streams_closing() => {}
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
