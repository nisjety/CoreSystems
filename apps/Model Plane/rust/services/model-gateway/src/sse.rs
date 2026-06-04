//! SSE streaming handler for model-gateway.
//!
//! Streams inference chunks as Server-Sent Events to HTTP clients.

use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
    Extension, Json,
};
use chrono::Utc;
use futures::Stream;
use mp_contracts::model_plane::v1::{
    orchestration_event, ApprovalKind, ApprovalState, ChatMessage, InferRequest,
    OrchestrationEvent, PlanState, StreamRunEventsRequest, SubagentRole, TodoState,
};
use mp_events::{envelope::Envelope, publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use serde::Serialize;
use serde_json::{json, Value};
use tokio_stream::{wrappers::ReceiverStream, StreamExt as _};
use tracing::info;

use crate::{auth::Claims, gateway_metrics, http_routes::InvokeRequest, state::AppState};

/// A single chunk in the SSE stream.
#[derive(Debug, Serialize)]
pub struct SseChunk {
    pub request_id: String,
    pub delta: String,
    pub done: bool,
    pub model_used: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

type HttpJsonError = (StatusCode, Json<Value>);

/// SSE streaming invoke handler.
///
/// Emits `STREAM_OPENED` on start, streams gRPC inference chunks,
/// a final `event: done` sentinel, and then `STREAM_CLOSED` + `USAGE_ENVELOPE`.
#[allow(clippy::too_many_lines)]
pub async fn invoke_stream_sse(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    axum::Json(req): axum::Json<InvokeRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let request_id = new_ulid();
    let model = req.model.clone().unwrap_or_else(|| "default".to_owned());
    let org_id = claims.org_id.clone();
    let user_id = claims.user_id.clone();

    // Resolve the harness profile → approval posture. Recorded on the opened
    // envelope so the run loop and operator surfaces agree (HARNESS_PHASE1 §1).
    let profile = crate::profile::AgentProfile::from_wire(req.profile.as_deref());
    let posture = profile.posture().as_permission_mode();

    let start = std::time::Instant::now();

    // Emit STREAM_OPENED event
    let mut open_envelope =
        build_stream_envelope(&request_id, "STREAM_OPENED", &org_id, &user_id, &model);
    if let Value::Object(ref mut map) = open_envelope.payload {
        map.insert("profile".to_owned(), json!(profile.as_wire()));
        map.insert("permission_mode".to_owned(), json!(posture));
    }
    if let Err(e) = state
        .publisher
        .publish(&subjects::stream_subject("opened"), &open_envelope)
        .await
    {
        tracing::warn!(error = %e, "failed to publish STREAM_OPENED");
    }
    gateway_metrics::stream_opened();

    let publisher = state.publisher.clone();
    let stream_buffers = state.stream_buffers.clone();
    let req_id = request_id.clone();
    let org_clone = org_id.clone();
    let user_clone = user_id.clone();
    let model_clone = model.clone();
    // chat-parity §2: opt-in rich SSE event families. Empty = plain path.
    let features = req.features.clone();

    // chat-parity §8: RAG grounding via Data Plane v2 retrieval (reused — no
    // new RAG store). When the request opts in, retrieve context, prepend it as
    // a system message, and carry the sources to emit as `citation` events.
    // Degrades to ungrounded chat if retrieval is unavailable.
    let grounding = if crate::retrieval::wants_grounding(&features) {
        crate::retrieval::retrieve(&state, &org_id, &user_id, &req.content).await
    } else {
        None
    };
    let (context_block, citations) = match grounding {
        Some(g) => (g.context_block, g.citations),
        None => (String::new(), Vec::new()),
    };

    let mut messages = Vec::new();
    if !context_block.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_owned(),
            content: context_block,
            name: String::new(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: req.content.clone(),
        name: String::new(),
    });

    let grpc_req = InferRequest {
        request_id: request_id.clone(),
        org_id: org_id.clone(),
        model: model.clone(),
        provider_hint: String::new(),
        messages,
        temperature: 0.7,
        max_tokens: 1024,
        structured_output_schema: req.structured_output_schema.clone().unwrap_or_default(),
        zdr: req.zdr,
    };

    // Clone the request so a streaming failure can retry via the (working)
    // non-streaming Infer fallback below.
    let grpc_req_fallback = grpc_req.clone();
    let grpc_response = state
        .inference_client
        .clone()
        .infer_stream(tonic::Request::new(grpc_req))
        .await;

    let mut grpc_stream = match grpc_response {
        Ok(response) => response.into_inner(),
        Err(e) => {
            // Streaming RPC unavailable. Do NOT emit a bare `done` — that reads
            // as a successful *empty* completion and forces every client to work
            // around it. Fall back to the non-streaming Infer (which works) and
            // reveal its real content in chunks: one robust endpoint, no
            // per-client fallback duplication. If Infer also fails, the fallback
            // emits an honest `error` event rather than a fake `done`.
            tracing::warn!(
                error = %e,
                request_id = %request_id,
                "infer_stream unavailable; falling back to non-streaming Infer"
            );
            return infer_fallback_stream(
                state.clone(),
                grpc_req_fallback,
                request_id,
                org_id,
                user_id,
                model,
                start,
                features,
                citations,
            );
        }
    };

    // chat-parity §4: register this stream so POST /v1/invoke/{id}/cancel can
    // stop it cooperatively. `cancels` is moved into the task to finish() on end.
    let cancels = state.cancels.clone();
    let cancel_flag = cancels.register(&request_id);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        // chat-parity §8: emit retrieved sources up front (gated on the
        // `citations` family) so the UI can render the Sources panel before
        // the answer streams in.
        for c in citations {
            let cite = crate::sse_events::ChatEvent::Citation {
                id: c.id,
                title: c.title,
                url: c.url,
                snippet: c.snippet,
            };
            if cite.should_emit(&features) {
                let _ = tx.send(Ok(cite.to_sse(&req_id))).await;
            }
        }

        // Per-request sequence index used as the SSE `id:` field so a
        // reconnecting client can send `Last-Event-Id` and resume from the
        // next delta (replay endpoint lands in Phase 2 — see
        // docs/HARNESS_PHASE1.md §3b).
        let mut seq: u64 = 0;
        while let Some(result) = grpc_stream.next().await {
            // chat-parity §4: cooperative cancel — the cancel endpoint flipped
            // this flag; emit a terminal `stopped` and end the stream.
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                let stopped = crate::sse_events::ChatEvent::Stopped {
                    reason: "client cancelled".to_owned(),
                };
                let _ = tx.send(Ok(stopped.to_sse(&req_id))).await;
                break;
            }
            match result {
                Ok(chunk) if !chunk.done => {
                    let sse_chunk = SseChunk {
                        request_id: chunk.request_id.clone(),
                        delta: chunk.delta.clone(),
                        done: false,
                        model_used: chunk.model_used.clone(),
                        input_tokens: 0,
                        output_tokens: 0,
                    };
                    let data = serde_json::to_string(&sse_chunk).unwrap_or_default();
                    // Buffer the delta for resumability before sending so a
                    // reconnect never races ahead of what we retained.
                    stream_buffers.append(&req_id, seq, &chunk.delta).await;
                    let _ = tx
                        .send(Ok(Event::default()
                            .id(seq.to_string())
                            .event("chunk")
                            .data(data)))
                        .await;
                    seq += 1;
                }
                Ok(chunk) => {
                    let input_tokens = u32::try_from(chunk.input_tokens).unwrap_or(0);
                    let output_tokens = u32::try_from(chunk.output_tokens).unwrap_or(0);
                    let model_used = if chunk.model_used.is_empty() {
                        model_clone.clone()
                    } else {
                        chunk.model_used.clone()
                    };
                    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

                    let close_envelope = build_stream_envelope(
                        &req_id,
                        "STREAM_CLOSED",
                        &org_clone,
                        &user_clone,
                        &model_used,
                    );
                    if let Err(e) = publisher
                        .publish(&subjects::stream_subject("closed"), &close_envelope)
                        .await
                    {
                        tracing::warn!(error = %e, "failed to publish STREAM_CLOSED");
                    }

                    let usage_envelope = build_usage_envelope(
                        &req_id,
                        &org_clone,
                        &user_clone,
                        &model_used,
                        input_tokens,
                        output_tokens,
                        latency_ms,
                    );
                    if let Err(e) = publisher
                        .publish(&subjects::usage_subject(&org_clone), &usage_envelope)
                        .await
                    {
                        tracing::warn!(error = %e, "failed to publish USAGE_ENVELOPE");
                    }
                    gateway_metrics::stream_closed();

                    info!(
                        request_id = %req_id,
                        latency_ms = latency_ms,
                        "SSE stream completed"
                    );

                    // Record terminal state so a late resumer still receives a
                    // correct `done` (lost-final-chunk handling, §3b).
                    stream_buffers
                        .finish(
                            &req_id,
                            crate::stream_buffer::StreamDone {
                                seq,
                                model_used: model_used.clone(),
                                input_tokens,
                                output_tokens,
                            },
                        )
                        .await;

                    // chat-parity §17: opt-in usage event (real tokens + latency)
                    // before the terminal done. cost_usd/confidence are wired in
                    // later slices (cost-core / reasoning) — null until then,
                    // never faked.
                    let usage_event = crate::sse_events::ChatEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cost_usd: None,
                        latency_ms,
                        confidence: None,
                    };
                    if usage_event.should_emit(&features) {
                        let _ = tx.send(Ok(usage_event.to_sse(&req_id))).await;
                    }

                    let done_chunk = SseChunk {
                        request_id: req_id.clone(),
                        delta: String::new(),
                        done: true,
                        model_used,
                        input_tokens,
                        output_tokens,
                    };
                    let data = serde_json::to_string(&done_chunk).unwrap_or_default();
                    let _ = tx
                        .send(Ok(Event::default()
                            .id(seq.to_string())
                            .event("done")
                            .data(data)))
                        .await;
                    break;
                }
                Err(e) => {
                    tracing::error!(error = %e, request_id = %req_id, "gRPC stream error");
                    break;
                }
            }
        }
        // chat-parity §4: stop tracking this stream for cancellation.
        cancels.finish(&req_id);
    });

    Sse::new(ReceiverStream::new(rx))
}

/// Split `text` into streaming-friendly pieces (~`target` chars, broken at
/// whitespace where possible) so the non-streaming Infer fallback reveals
/// content progressively instead of in one blob. Rejoining the pieces
/// reproduces `text` exactly — no content is added or dropped.
fn chunk_for_stream(text: &str, target: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in text.split_inclusive(char::is_whitespace) {
        if !cur.is_empty() && cur.len() + word.len() > target {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Fallback SSE stream used when `InferStream` is unavailable: call the
/// (working) non-streaming `Infer` and reveal its content in chunks so
/// `/v1/invoke/stream` still returns real tokens. If `Infer` ALSO fails, emit
/// an honest `error` event — never a fake successful `done`.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)] // cohesive streaming emission, mirrors invoke_stream_sse
fn infer_fallback_stream(
    state: AppState,
    grpc_req: InferRequest,
    request_id: String,
    org_id: String,
    user_id: String,
    model: String,
    start: std::time::Instant,
    features: Vec<String>,
    citations: Vec<crate::retrieval::GroundingCitation>,
) -> Sse<ReceiverStream<Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);
    tokio::spawn(async move {
        // chat-parity §8: surface retrieved sources before the answer (gated on
        // the `citations` family), mirroring the streaming path.
        for c in citations {
            let cite = crate::sse_events::ChatEvent::Citation {
                id: c.id,
                title: c.title,
                url: c.url,
                snippet: c.snippet,
            };
            if cite.should_emit(&features) {
                let _ = tx.send(Ok(cite.to_sse(&request_id))).await;
            }
        }

        let publisher = state.publisher.clone();
        let buffers = state.stream_buffers.clone();
        let result = state
            .inference_client
            .clone()
            .infer(tonic::Request::new(grpc_req))
            .await;
        let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        match result {
            Ok(resp) => {
                let resp = resp.into_inner();
                let model_used = if resp.model_used.is_empty() {
                    model.clone()
                } else {
                    resp.model_used.clone()
                };
                let input_tokens = u32::try_from(resp.input_tokens).unwrap_or(0);
                let output_tokens = u32::try_from(resp.output_tokens).unwrap_or(0);

                let mut seq: u64 = 0;
                for piece in chunk_for_stream(&resp.content, 48) {
                    buffers.append(&request_id, seq, &piece).await;
                    let sse_chunk = SseChunk {
                        request_id: request_id.clone(),
                        delta: piece,
                        done: false,
                        model_used: model_used.clone(),
                        input_tokens: 0,
                        output_tokens: 0,
                    };
                    let data = serde_json::to_string(&sse_chunk).unwrap_or_default();
                    if tx
                        .send(Ok(Event::default()
                            .id(seq.to_string())
                            .event("chunk")
                            .data(data)))
                        .await
                        .is_err()
                    {
                        return; // client disconnected
                    }
                    seq += 1;
                }

                let close = build_stream_envelope(
                    &request_id,
                    "STREAM_CLOSED",
                    &org_id,
                    &user_id,
                    &model_used,
                );
                let _ = publisher
                    .publish(&subjects::stream_subject("closed"), &close)
                    .await;
                let usage = build_usage_envelope(
                    &request_id,
                    &org_id,
                    &user_id,
                    &model_used,
                    input_tokens,
                    output_tokens,
                    latency_ms,
                );
                let _ = publisher
                    .publish(&subjects::usage_subject(&org_id), &usage)
                    .await;
                gateway_metrics::stream_closed();
                buffers
                    .finish(
                        &request_id,
                        crate::stream_buffer::StreamDone {
                            seq,
                            model_used: model_used.clone(),
                            input_tokens,
                            output_tokens,
                        },
                    )
                    .await;

                // chat-parity §17: opt-in usage event (real tokens + latency).
                let usage_event = crate::sse_events::ChatEvent::Usage {
                    input_tokens,
                    output_tokens,
                    cost_usd: None,
                    latency_ms,
                    confidence: None,
                };
                if usage_event.should_emit(&features) {
                    let _ = tx.send(Ok(usage_event.to_sse(&request_id))).await;
                }

                let done_chunk = SseChunk {
                    request_id: request_id.clone(),
                    delta: String::new(),
                    done: true,
                    model_used,
                    input_tokens,
                    output_tokens,
                };
                let data = serde_json::to_string(&done_chunk).unwrap_or_default();
                let _ = tx
                    .send(Ok(Event::default()
                        .id(seq.to_string())
                        .event("done")
                        .data(data)))
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    request_id = %request_id,
                    "infer fallback also failed; emitting error event"
                );
                let close =
                    build_stream_envelope(&request_id, "STREAM_CLOSED", &org_id, &user_id, &model);
                let _ = publisher
                    .publish(&subjects::stream_subject("closed"), &close)
                    .await;
                gateway_metrics::stream_closed();
                // chat-parity §20: structured error — stable `code` + `retryable`
                // so the client can branch (the `message` field is preserved for
                // back-compat with existing error handlers).
                let err_evt = crate::sse_events::ChatEvent::Error {
                    code: "model_plane_unavailable".to_owned(),
                    message: e.message().to_owned(),
                    retryable: true,
                };
                let _ = tx.send(Ok(err_evt.to_sse(&request_id))).await;
            }
        }
    });
    Sse::new(ReceiverStream::new(rx))
}

#[cfg(test)]
mod fallback_tests {
    use super::chunk_for_stream;

    #[test]
    fn chunk_for_stream_is_lossless_and_splits() {
        let text = "the quick brown fox jumps over the lazy dog";
        let chunks = chunk_for_stream(text, 12);
        assert!(chunks.len() > 1, "should split into multiple pieces");
        assert_eq!(
            chunks.concat(),
            text,
            "rejoin must reproduce the input exactly"
        );
        assert!(chunks.iter().all(|c| !c.is_empty()));
    }

    #[test]
    fn chunk_for_stream_empty_yields_nothing() {
        assert!(chunk_for_stream("", 10).is_empty());
    }

    #[test]
    fn chunk_for_stream_short_is_single_piece() {
        assert_eq!(chunk_for_stream("hello", 100), vec!["hello".to_owned()]);
    }

    #[test]
    fn chunk_for_stream_keeps_overlong_word_whole() {
        let word = "supercalifragilisticexpialidocious";
        assert_eq!(chunk_for_stream(word, 5), vec![word.to_owned()]);
    }
}

/// Resume a chat stream after a reconnect/reload (`HARNESS_PHASE1` §3b).
///
/// Replays buffered deltas with `seq > Last-Event-Id` and, when the original
/// stream has finished, the terminal `done` event. Returns 404 when the
/// `request_id` is unknown (evicted past TTL or never existed) so the client
/// restarts the request instead of silently hanging.
///
/// # Errors
///
/// Returns a 404 `HttpJsonError` when `request_id` is unknown (evicted past TTL or
/// never existed).
pub async fn invoke_resume_sse(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    Extension(_claims): Extension<Claims>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpJsonError> {
    let after_seq = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let replay = state
        .stream_buffers
        .replay_after(&request_id, after_seq)
        .await;
    if !replay.found {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "stream not resumable; restart the request" })),
        ));
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);
    let req_id = request_id.clone();
    tokio::spawn(async move {
        for delta in replay.deltas {
            let chunk = SseChunk {
                request_id: req_id.clone(),
                delta: delta.delta,
                done: false,
                model_used: String::new(),
                input_tokens: 0,
                output_tokens: 0,
            };
            let data = serde_json::to_string(&chunk).unwrap_or_default();
            let _ = tx
                .send(Ok(Event::default()
                    .id(delta.seq.to_string())
                    .event("chunk")
                    .data(data)))
                .await;
        }
        if let Some(done) = replay.done {
            let chunk = SseChunk {
                request_id: req_id.clone(),
                delta: String::new(),
                done: true,
                model_used: done.model_used,
                input_tokens: done.input_tokens,
                output_tokens: done.output_tokens,
            };
            let data = serde_json::to_string(&chunk).unwrap_or_default();
            let _ = tx
                .send(Ok(Event::default()
                    .id(done.seq.to_string())
                    .event("done")
                    .data(data)))
                .await;
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

/// Streams orchestration run events as Server-Sent Events.
///
/// # Errors
///
/// Returns an `HttpJsonError` if opening the upstream `stream_run_events` gRPC
/// stream fails.
pub async fn run_events_sse(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Extension(_claims): Extension<Claims>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpJsonError> {
    // Resume cursor: the browser's EventSource auto-sends `Last-Event-Id` on
    // reconnect. Forward it so session-core replays buffered events after that
    // id, then tails live (docs/HARNESS_PHASE1.md §3a). Absent on first connect.
    let after_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let response = state
        .orchestration_client
        .clone()
        .stream_run_events(StreamRunEventsRequest {
            run_id: run_id.clone(),
            after_event_id,
        })
        .await
        .map_err(|e| grpc_status_to_http(&e))?;

    let mut grpc_stream = response.into_inner();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        while let Some(result) = grpc_stream.next().await {
            match result {
                Ok(event) => {
                    // Carry the server-assigned event_id onto the SSE `id:` line
                    // so the next reconnect resumes from exactly here.
                    let event_id = event.event_id.clone();
                    if let Some(sse_event) = orchestration_event_to_sse(&event) {
                        let sse_event = if event_id.is_empty() {
                            sse_event
                        } else {
                            sse_event.id(event_id)
                        };
                        let _ = tx.send(Ok(sse_event)).await;
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, run_id = %run_id, "orchestration run event stream closed with error");
                    break;
                }
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

fn build_stream_envelope(
    request_id: &str,
    event_type: &str,
    org_id: &str,
    user_id: &str,
    model: &str,
) -> Envelope {
    Envelope {
        event_id: new_ulid(),
        event_type: event_type.to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: format!("{request_id}-{event_type}"),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({ "model": model }),
        zdr: false,
    }
}

fn build_usage_envelope(
    request_id: &str,
    org_id: &str,
    user_id: &str,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    latency_ms: u64,
) -> Envelope {
    Envelope {
        event_id: new_ulid(),
        event_type: "USAGE_ENVELOPE".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: request_id.to_owned(),
        causation_id: String::new(),
        idempotency_key: format!("{request_id}-USAGE_ENVELOPE"),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("request/{request_id}"),
        payload: serde_json::json!({
            "request_id": request_id,
            "org_id": org_id,
            "user_id": user_id,
            "model": model,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "latency_ms": latency_ms,
        }),
        zdr: false,
    }
}

fn grpc_status_to_http(error: &tonic::Status) -> HttpJsonError {
    let status = match error.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        tonic::Code::Unavailable => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (status, Json(json!({ "error": error.message() })))
}

fn orchestration_event_to_sse(event: &OrchestrationEvent) -> Option<Event> {
    let payload = event_payload_value(event)?;
    let event_name = match event.event.as_ref()? {
        orchestration_event::Event::PlanTransitioned(_) => "plan_transitioned",
        orchestration_event::Event::TodoTransitioned(_) => "todo_transitioned",
        orchestration_event::Event::ApprovalStateChanged(_) => "approval_state_changed",
        orchestration_event::Event::SubagentAttached(_) => "subagent_attached",
        orchestration_event::Event::SubagentStopped(_) => "subagent_stopped",
        orchestration_event::Event::RunPausedForApproval(_) => "run_paused_for_approval",
        orchestration_event::Event::RunResumedAfterApproval(_) => "run_resumed_after_approval",
    };
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
    Some(Event::default().event(event_name).data(data))
}

fn event_payload_value(event: &OrchestrationEvent) -> Option<Value> {
    let mut object = serde_json::Map::new();
    if let Some(at) = event.at.as_ref() {
        object.insert(
            "at".to_owned(),
            json!({ "seconds": at.seconds, "nanos": at.nanos }),
        );
    }

    match event.event.as_ref()? {
        orchestration_event::Event::PlanTransitioned(payload) => {
            object.insert("plan_id".to_owned(), json!(payload.plan_id));
            object.insert("run_id".to_owned(), json!(payload.run_id));
            object.insert(
                "from".to_owned(),
                json!(enum_name(PlanState::try_from(payload.from).ok().as_ref())),
            );
            object.insert(
                "to".to_owned(),
                json!(enum_name(PlanState::try_from(payload.to).ok().as_ref())),
            );
        }
        orchestration_event::Event::TodoTransitioned(payload) => {
            object.insert("todo_id".to_owned(), json!(payload.todo_id));
            object.insert("thread_id".to_owned(), json!(payload.thread_id));
            object.insert(
                "from".to_owned(),
                json!(enum_name(TodoState::try_from(payload.from).ok().as_ref())),
            );
            object.insert(
                "to".to_owned(),
                json!(enum_name(TodoState::try_from(payload.to).ok().as_ref())),
            );
        }
        orchestration_event::Event::ApprovalStateChanged(payload) => {
            object.insert("approval_id".to_owned(), json!(payload.approval_id));
            object.insert("run_id".to_owned(), json!(payload.run_id));
            object.insert(
                "approval_kind".to_owned(),
                json!(enum_name(
                    ApprovalKind::try_from(payload.approval_kind).ok().as_ref()
                )),
            );
            object.insert(
                "to".to_owned(),
                json!(enum_name(ApprovalState::try_from(payload.to).ok().as_ref())),
            );
            object.insert("decided_by".to_owned(), json!(payload.decided_by));
        }
        orchestration_event::Event::SubagentAttached(payload) => {
            object.insert("parent_run_id".to_owned(), json!(payload.parent_run_id));
            object.insert("child_run_id".to_owned(), json!(payload.child_run_id));
            object.insert(
                "role".to_owned(),
                json!(enum_name(
                    SubagentRole::try_from(payload.role).ok().as_ref()
                )),
            );
        }
        orchestration_event::Event::SubagentStopped(payload) => {
            object.insert("child_run_id".to_owned(), json!(payload.child_run_id));
            object.insert("status".to_owned(), json!(payload.status));
        }
        orchestration_event::Event::RunPausedForApproval(payload) => {
            object.insert("run_id".to_owned(), json!(payload.run_id));
            object.insert("approval_id".to_owned(), json!(payload.approval_id));
        }
        orchestration_event::Event::RunResumedAfterApproval(payload) => {
            object.insert("run_id".to_owned(), json!(payload.run_id));
            object.insert("approval_id".to_owned(), json!(payload.approval_id));
        }
    }

    Some(Value::Object(object))
}

fn enum_name<E>(value: Option<&E>) -> &'static str
where
    E: EnumName,
{
    value.map_or("UNSPECIFIED", EnumName::as_str_name)
}

trait EnumName {
    fn as_str_name(&self) -> &'static str;
}

impl EnumName for PlanState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for TodoState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for ApprovalKind {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for ApprovalState {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

impl EnumName for SubagentRole {
    fn as_str_name(&self) -> &'static str {
        self.as_str_name()
    }
}

#[cfg(test)]
mod tests {
    use super::{build_stream_envelope, build_usage_envelope};

    // Golden parity: every envelope the gateway emits derives its
    // `correlation_id` from the per-request id, identically across the HTTP/SSE
    // and gRPC paths (grpc.rs sets `correlation_id: request_id` the same way).
    // These lock that contract so a future divergence between transports — the
    // classic cause of un-traceable runs — fails the build.

    #[test]
    fn stream_envelope_correlation_id_equals_request_id() {
        let env = build_stream_envelope("req-ABC", "STREAM_OPENED", "org-1", "user-1", "gpt-4o");
        assert_eq!(env.correlation_id, "req-ABC");
        assert_eq!(env.producer, "model-gateway");
        // idempotency must be derivable from the same request id.
        assert!(env.idempotency_key.contains("req-ABC"));
        // resource_ref ties the event to the request, not a random id.
        assert!(env.resource_ref.contains("req-ABC"));
    }

    #[test]
    fn usage_envelope_correlation_id_equals_request_id() {
        let env = build_usage_envelope("req-ABC", "org-1", "user-1", "gpt-4o", 10, 20, 33);
        assert_eq!(env.correlation_id, "req-ABC");
        assert_eq!(env.producer, "model-gateway");
        assert!(env.idempotency_key.contains("req-ABC"));
    }

    #[test]
    fn stream_and_usage_envelopes_share_correlation_id_for_one_request() {
        // Parity across the two envelopes a single streamed request emits.
        let request_id = "req-PARITY-1";
        let opened = build_stream_envelope(request_id, "STREAM_OPENED", "o", "u", "m");
        let usage = build_usage_envelope(request_id, "o", "u", "m", 1, 1, 1);
        assert_eq!(opened.correlation_id, usage.correlation_id);
        assert_eq!(opened.correlation_id, request_id);
    }
}
