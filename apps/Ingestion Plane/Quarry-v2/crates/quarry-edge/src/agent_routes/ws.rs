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
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiErr> {
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
    // W3-E3.4-5: read after the owner check, so a caller who does not own the run gets the
    // plain 404 above; a view-only share of a ZDR run is refused here (403).
    let scope = owner_live_scope(&headers, zdr, &request_id)?;
    let session = frame_session(&state, &session, query.frame.tab_id.as_deref(), &request_id).await?;
    // A tab-addressed stream is an observer surface; it must not execute
    // socket input against the run's separately selected tab.
    let scope = if query.frame.tab_id.is_some() { LiveScope::ViewOnly } else { scope };
    let viewer = handle.live.viewer_guard();
    let options = live_frame_options(&query.frame, viewport)
        .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;
    let interval = stream_interval(&query);
    let max_frames = query.max_frames.unwrap_or(0).min(7_200);
    let zdr_active = matches!(zdr, ZdrMode::On);

    Ok(ws.on_upgrade(move |socket| {
        browser_ws_loop(
            socket, state, handle, claims, viewer, session, options, interval, max_frames, zdr_active,
            scope,
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
    viewer: ViewerGuard,
    session: BrowserSession,
    options: LiveFrameOptions,
    interval: Duration,
    max_frames: u64,
    zdr_active: bool,
    scope: LiveScope,
) {
    // Held for as long as the socket is open.
    let _viewer = viewer;
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
                        let _ = send_ws_json(&mut socket, ws_error_frame(err)).await;
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
            () = crate::drain::streams_closing() => {
                // WS6.J: the edge is shutting down.
                let _ = send_ws_json(&mut socket, serde_json::json!({
                    "type": "done",
                    "reason": "shutdown"
                })).await;
                break;
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<BrowserWsClientMessage>(&text) {
                            // W3-E3.4-5: a view-only connection answers an input frame with an
                            // error frame and keeps the socket open; the frame is not executed.
                            Ok(client_message) if !scope.accepts_client_message(&client_message) => {
                                if let Some(frame) = view_only_ws_refusal(scope, &client_message) {
                                    if send_ws_json(&mut socket, frame).await.is_err() {
                                        break;
                                    }
                                }
                            }
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
                                        let _ = send_ws_json(&mut socket, ws_error_frame(err)).await;
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = send_ws_json(
                                    &mut socket,
                                    ws_error_frame(QuarryError::new(
                                        ErrorCode::BadRequest,
                                        format!("invalid websocket message: {err}"),
                                    )),
                                )
                                .await;
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
                    Some(Err(_)) => {
                        // The frame error's text is not logged: it is a transport error and a
                        // ZDR caller's line would be dropped for carrying it.
                        tracing::debug!(error_class = "ws_receive", "browser websocket receive failed");
                        break;
                    }
                }
            }
        }
    }
}

/// The socket's error frame (O4, D1-022). It carries what the REST error body carries: the
/// code in its wire spelling (`USER_TOOK_OVER`, `FORBIDDEN`, ...), the message, the
/// retry hint and the details, so a client reads one vocabulary on both transports. The
/// frame used to send the error's `Display` (`UserTookOver: ...`), a Rust identifier and
/// the text unmasked; the message and the details now leave as REST sends them, masked
/// by `QuarryError::redacted`.
pub(super) fn ws_error_frame(err: QuarryError) -> serde_json::Value {
    let err = err.redacted();
    let mut frame = serde_json::json!({
        "type": "error",
        "code": err.code.wire_name(),
        "message": err.message,
        "retryable": err.retryable,
    });
    if let Some(details) = err.details {
        frame["details"] = details;
    }
    frame
}

pub(super) async fn send_ws_json(
    socket: &mut WebSocket,
    value: serde_json::Value,
) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string())).await
}

#[cfg(test)]
mod ws_error_frame_tests {
    use super::*;

    #[test]
    fn the_frame_carries_the_rest_code_the_message_and_the_details() {
        let frame = ws_error_frame(
            QuarryError::new(ErrorCode::UserTookOver, "the person has taken control")
                .with_details(serde_json::json!({ "control_mode": "human_takeover", "control_epoch": 1 })),
        );
        assert_eq!(frame["type"], "error");
        assert_eq!(frame["code"], "USER_TOOK_OVER");
        assert_eq!(frame["message"], "the person has taken control");
        assert_eq!(frame["retryable"], false);
        assert_eq!(frame["details"]["control_epoch"], 1);
        // The Rust identifier of the code is not on the wire.
        assert!(!frame.to_string().contains("UserTookOver"), "{frame}");
    }

    #[test]
    fn a_frame_without_details_has_no_details_key() {
        let frame = ws_error_frame(QuarryError::new(ErrorCode::BadRequest, "invalid websocket message"));
        assert_eq!(frame["code"], "BAD_REQUEST");
        assert!(frame.get("details").is_none(), "{frame}");
    }
}
