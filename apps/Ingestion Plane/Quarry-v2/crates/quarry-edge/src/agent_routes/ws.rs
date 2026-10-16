use super::*;

/// `GET /v1/agent/runs/{run_id}/frames/ws` — bidirectional browser channel.
///
/// Server sends transient `frame` events. Client may send
/// `{ "type": "action", "action": ... }` to execute a normal Quarry
/// browser action and receive an `observation` event. Frames are never
/// persisted; action observations still use the existing artifact-backed
/// `ObservationRunner` path so replay/debugging stays deterministic.
pub async fn live_frame_ws(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
    Query(query): Query<FrameStreamQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiErr> {
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
    let zdr_active = matches!(zdr, ZdrMode::On);

    Ok(ws.on_upgrade(move |socket| {
        browser_ws_loop(
            socket, state, handle, claims, session, options, interval, max_frames, zdr_active,
        )
    }))
}

// reason: websocket handler glue — every argument is a distinct, already
// clamped/authorized concern handed over from `live_frame_ws`; bundling
// them into a one-off struct would only move the argument list.
#[allow(clippy::too_many_arguments)]
pub(super) async fn browser_ws_loop(
    mut socket: WebSocket,
    state: AppState,
    handle: RunHandle,
    claims: crate::auth::Claims,
    session: BrowserSession,
    options: LiveFrameOptions,
    interval: Duration,
    max_frames: u64,
    zdr_active: bool,
) {
    let mut ticker = tokio::time::interval(interval);
    let mut sequence = 0_u64;
    let mut devtools_sequence = 0_u64;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if max_frames > 0 && sequence >= max_frames {
                    let _ = send_ws_json(&mut socket, serde_json::json!({
                        "type": "done",
                        "reason": "max_frames"
                    })).await;
                    break;
                }

                match state.agent_driver.live_frame(&session, options).await {
                    Ok(frame) => {
                        sequence = sequence.saturating_add(1);
                        if send_ws_json(&mut socket, serde_json::json!({
                            "type": "frame",
                            "sequence": sequence,
                            "mimeType": frame.mime_type,
                            "dataBase64": frame.data_base64,
                            "zdr": zdr_active
                        })).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = send_ws_json(&mut socket, serde_json::json!({
                            "type": "error",
                            "message": err.to_string()
                        })).await;
                        break;
                    }
                }

                if let Ok(events) = state
                    .agent_driver
                    .devtools_events(&session, devtools_sequence, 100)
                    .await
                {
                    if let Some(last) = events.last() {
                        devtools_sequence = last.sequence;
                    }
                    if !events.is_empty()
                        && send_ws_json(&mut socket, serde_json::json!({
                            "type": "devtools",
                            "events": events,
                            "zdr": zdr_active
                        })).await.is_err()
                    {
                        break;
                    }
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<BrowserWsClientMessage>(&text) {
                            Ok(BrowserWsClientMessage::Ping) => {
                                if send_ws_json(&mut socket, serde_json::json!({ "type": "pong" })).await.is_err() {
                                    break;
                                }
                            }
                            Ok(BrowserWsClientMessage::Action { action, instruction, actor }) => {
                                match execute_ws_action(&state, &handle, &claims, action, instruction, actor).await {
                                    Ok(observation) => {
                                        if send_ws_json(&mut socket, serde_json::json!({
                                            "type": "observation",
                                            "observation": observation
                                        })).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(err) => {
                                        let _ = send_ws_json(&mut socket, serde_json::json!({
                                            "type": "error",
                                            "message": err.to_string()
                                        })).await;
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = send_ws_json(&mut socket, serde_json::json!({
                                    "type": "error",
                                    "message": format!("invalid websocket message: {err}")
                                })).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        tracing::debug!(error = %err, "browser websocket receive failed");
                        break;
                    }
                }
            }
        }
    }
}

pub(super) async fn send_ws_json(
    socket: &mut WebSocket,
    value: serde_json::Value,
) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string())).await
}
