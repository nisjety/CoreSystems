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

    let grpc_req = InferRequest {
        request_id: request_id.clone(),
        org_id: org_id.clone(),
        model: model.clone(),
        provider_hint: String::new(),
        messages: vec![ChatMessage {
            role: "user".to_owned(),
            content: req.content.clone(),
            name: String::new(),
        }],
        temperature: 0.7,
        max_tokens: 1024,
        structured_output_schema: req.structured_output_schema.clone().unwrap_or_default(),
        zdr: req.zdr,
    };

    let grpc_response = state
        .inference_client
        .clone()
        .infer_stream(tonic::Request::new(grpc_req))
        .await;

    let mut grpc_stream = match grpc_response {
        Ok(response) => response.into_inner(),
        Err(e) => {
            tracing::error!(error = %e, request_id = %request_id, "infer_stream failed");

            let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let close_envelope =
                build_stream_envelope(&request_id, "STREAM_CLOSED", &org_id, &user_id, &model);
            if let Err(pub_err) = state
                .publisher
                .publish(&subjects::stream_subject("closed"), &close_envelope)
                .await
            {
                tracing::warn!(error = %pub_err, "failed to publish STREAM_CLOSED");
            }
            gateway_metrics::stream_closed();

            let usage_envelope =
                build_usage_envelope(&request_id, &org_id, &user_id, &model, 0, 0, latency_ms);
            if let Err(pub_err) = state
                .publisher
                .publish(&subjects::usage_subject(&org_id), &usage_envelope)
                .await
            {
                tracing::warn!(error = %pub_err, "failed to publish USAGE_ENVELOPE");
            }

            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(1);
            let done_chunk = SseChunk {
                request_id,
                delta: String::new(),
                done: true,
                model_used: model,
                input_tokens: 0,
                output_tokens: 0,
            };
            let data = serde_json::to_string(&done_chunk).unwrap_or_default();
            let _ = tx
                .send(Ok(Event::default().id("0").event("done").data(data)))
                .await;

            return Sse::new(ReceiverStream::new(rx));
        }
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        // Per-request sequence index used as the SSE `id:` field so a
        // reconnecting client can send `Last-Event-Id` and resume from the
        // next delta (replay endpoint lands in Phase 2 — see
        // docs/HARNESS_PHASE1.md §3b).
        let mut seq: u64 = 0;
        while let Some(result) = grpc_stream.next().await {
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
                    let input_tokens = chunk.input_tokens as u32;
                    let output_tokens = chunk.output_tokens as u32;
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
    });

    Sse::new(ReceiverStream::new(rx))
}

/// Resume a chat stream after a reconnect/reload (HARNESS_PHASE1 §3b).
///
/// Replays buffered deltas with `seq > Last-Event-Id` and, when the original
/// stream has finished, the terminal `done` event. Returns 404 when the
/// `request_id` is unknown (evicted past TTL or never existed) so the client
/// restarts the request instead of silently hanging.
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
        .map_err(grpc_status_to_http)?;

    let mut grpc_stream = response.into_inner();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        while let Some(result) = grpc_stream.next().await {
            match result {
                Ok(event) => {
                    // Carry the server-assigned event_id onto the SSE `id:` line
                    // so the next reconnect resumes from exactly here.
                    let event_id = event.event_id.clone();
                    if let Some(sse_event) = orchestration_event_to_sse(event) {
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

fn grpc_status_to_http(error: tonic::Status) -> HttpJsonError {
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

fn orchestration_event_to_sse(event: OrchestrationEvent) -> Option<Event> {
    let payload = event_payload_value(&event)?;
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
