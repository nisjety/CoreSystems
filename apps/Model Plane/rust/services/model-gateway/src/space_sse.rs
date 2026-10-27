//! Space room event stream (build plan WS02-S3).
//!
//! `GET /v1/spaces/:space_id/events` relays Session Core's `SpaceEvents.StreamSpaceEvents` as
//! Server-Sent Events. The frames carry ids and closed words only (never message text, titles,
//! previews or outputs), so a client learns THAT something changed and refetches through the
//! authorized read path. The vocabulary is its own (`connected`, `space_event`, `heartbeat`,
//! `cursor_too_old`, `closing`, `error`) and is pinned by the golden
//! `apps/Model Plane/api/contracts/space-events.json`; it is NOT part of the run-event SSE
//! taxonomy in `sse_events.rs`, which this module does not touch.
//!
//! Authority is Session Core's: this route forwards the caller's verified bearer plus the Control
//! decision (`x-space-decision-ref` / `x-space-decision-token`, headers so they stay out of access
//! logs) and holds no authority of its own. User principals only: a service principal is refused
//! by `authorize_principal_route` for every path outside its small allowlist. The gRPC stream is
//! opened BEFORE the response head is committed, so a refusal is an HTTP status, not a frame.
//!
//! Errors, as `{error:{code,message,details}}`: `UNAUTHENTICATED` 401 `unauthorized`,
//! `PERMISSION_DENIED` 403 `space_stream_forbidden`, `INVALID_ARGUMENT` 400 `invalid_request`,
//! `RESOURCE_EXHAUSTED` 503 `space_stream_capacity` (with `Retry-After`), anything else 503
//! `space_stream_unavailable`. These are not ZDR refusals, so the typed catalogue is not used.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
    Extension, Json, Router,
};
use futures::Stream;
use mp_contracts::model_plane::v1::{
    space_event_frame::Frame, SpaceEventFrame, SpaceEventRecord, StreamSpaceEventsRequest,
};
use serde_json::{json, Map, Value};
use tonic::Streaming;

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::authenticated_session_request;
use crate::state::AppState;

const DECISION_REF_HEADER: &str = "x-space-decision-ref";
const DECISION_TOKEN_HEADER: &str = "x-space-decision-token";
const MAX_SPACE_ID_BYTES: usize = 128;
const KEEP_ALIVE: Duration = Duration::from_secs(15);
const CAPACITY_RETRY_AFTER_SECONDS: &str = "2";

/// The Space event stream routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/spaces/:space_id/events", get(space_events))
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}

/// The HTTP form of an upstream refusal. Messages are fixed words, never the upstream text.
fn status_response(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_stream_forbidden",
            "the Space event stream is not permitted",
        ),
        tonic::Code::InvalidArgument => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the Space event stream request is invalid",
        ),
        tonic::Code::ResourceExhausted => {
            let mut response = error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_stream_capacity",
                "the Space event stream is at capacity",
            );
            response.headers_mut().insert(
                header::RETRY_AFTER,
                header::HeaderValue::from_static(CAPACITY_RETRY_AFTER_SECONDS),
            );
            response
        }
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_stream_unavailable",
            "the Space event stream is unavailable",
        ),
    }
}

/// `<generation>.<seq>`, digits only. `None` for anything else.
fn parse_cursor(value: &str) -> Option<(i64, i64)> {
    let (generation, seq) = value.split_once('.')?;
    let digits = |part: &str| !part.is_empty() && part.len() <= 18 && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(generation) || !digits(seq) {
        return None;
    }
    Some((generation.parse().ok()?, seq.parse().ok()?))
}

fn header_text(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_owned()
}

async fn space_events(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let space_id = space_id.trim().to_owned();
    if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "space_id is invalid");
    }
    // A user principal only: Session Core binds the stream to this user id.
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "space_stream_forbidden",
            "a user principal is required",
        );
    }
    let last_event_id = header_text(&headers, "last-event-id");
    let (resume, generation, after_seq) = if last_event_id.is_empty() {
        (false, 0, 0)
    } else {
        match parse_cursor(&last_event_id) {
            Some((generation, seq)) => (true, generation, seq),
            None => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Last-Event-ID is not a Space event cursor",
                )
            }
        }
    };

    let request = match authenticated_session_request(
        StreamSpaceEventsRequest {
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
            space_id: space_id.clone(),
            decision_ref: header_text(&headers, DECISION_REF_HEADER),
            decision_token: header_text(&headers, DECISION_TOKEN_HEADER),
            resume,
            generation,
            after_seq,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };

    // Open the upstream stream before returning, so a refusal is a status, not a frame.
    let upstream = match state.space_events_client.clone().stream_space_events(request).await {
        Ok(response) => response.into_inner(),
        Err(status) => return status_response(&status),
    };

    Sse::new(relay(upstream, space_id))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keepalive"))
        .into_response()
}

/// Forwards frames until the upstream ends. A mid-stream upstream failure becomes one `error`
/// event and the end of the stream; the client reconnects with its last id.
fn relay(
    upstream: Streaming<SpaceEventFrame>,
    space_id: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    futures::stream::unfold(Some(upstream), move |state| {
        let space_id = space_id.clone();
        async move {
            let mut upstream = state?;
            match upstream.message().await {
                Ok(Some(frame)) => {
                    let event = frame_event(&frame, &space_id)?;
                    Some((Ok(event), Some(upstream)))
                }
                Ok(None) => None,
                Err(status) => {
                    let code = match status.code() {
                        tonic::Code::Unauthenticated => "unauthorized",
                        tonic::Code::PermissionDenied => "space_stream_forbidden",
                        _ => "space_stream_unavailable",
                    };
                    let event = Event::default().event("error").data(
                        json!({ "error": { "code": code, "message": "the Space event stream ended", "details": {} } })
                            .to_string(),
                    );
                    Some((Ok(event), None))
                }
            }
        }
    })
}

fn cursor_id(generation: i64, cursor: i64) -> String {
    format!("{generation}.{cursor}")
}

/// The SSE form of one gRPC frame. `None` for an empty frame (dropped).
fn frame_event(frame: &SpaceEventFrame, space_id: &str) -> Option<Event> {
    let event = match frame.frame.as_ref()? {
        Frame::Connected(connected) => {
            let mut event = Event::default().event("connected").data(
                json!({
                    "v": 1,
                    "space_id": space_id,
                    "generation": connected.generation,
                    "head": connected.head,
                    "resumed": connected.resumed,
                    "lease_seconds": connected.lease_seconds,
                    "heartbeat_seconds": connected.heartbeat_seconds,
                    "mode": connected.mode,
                })
                .to_string(),
            );
            // A poll_only answer has no position to resume from.
            if connected.mode == "live" {
                event = event.id(cursor_id(connected.generation, connected.cursor));
            }
            event
        }
        Frame::Event(record) => Event::default()
            .event("space_event")
            .id(cursor_id(record.generation, record.seq))
            .data(space_event_json(record, space_id).to_string()),
        Frame::Heartbeat(heartbeat) => Event::default()
            .event("heartbeat")
            .id(cursor_id(heartbeat.generation, heartbeat.cursor))
            .data(json!({ "head": heartbeat.head, "generation": heartbeat.generation }).to_string()),
        Frame::CursorTooOld(too_old) => Event::default().event("cursor_too_old").data(
            json!({ "reason": too_old.reason, "head": too_old.head, "generation": too_old.generation })
                .to_string(),
        ),
        Frame::Closing(closing) => {
            let mut event = Event::default().event("closing").data(
                json!({
                    "code": closing.code,
                    "reconnect": closing.reconnect,
                    "retry_after_ms": closing.retry_after_ms,
                })
                .to_string(),
            );
            if closing.generation > 0 {
                event = event.id(cursor_id(closing.generation, closing.cursor));
            }
            event
        }
    };
    Some(event)
}

/// The `space_event` data object: ids and closed words only. Empty fields are omitted, and a
/// stub (`hidden`, `redacted`) carries no refs.
fn space_event_json(record: &SpaceEventRecord, space_id: &str) -> Value {
    let mut actor = Map::new();
    actor.insert("type".to_owned(), json!(record.actor_type));
    if !record.actor_id.is_empty() {
        actor.insert("id".to_owned(), json!(record.actor_id));
    }
    let mut refs = Map::new();
    let mut put = |key: &str, value: &str| {
        if !value.is_empty() {
            refs.insert(key.to_owned(), json!(value));
        }
    };
    if record.kind != "hidden" && record.kind != "redacted" {
        put("thread_id", &record.thread_id);
        put("run_id", &record.run_id);
        put("message_id", &record.message_id);
        put("role", &record.role);
        put("agent_id", &record.agent_id);
        put("state", &record.state);
        put("prev", &record.prev_state);
        put("origin", &record.origin);
        put("parent_run_id", &record.parent_run_id);
        if record.message_sequence > 0 {
            refs.insert("message_sequence".to_owned(), json!(record.message_sequence));
        }
        if !record.fields.is_empty() {
            refs.insert("fields".to_owned(), json!(record.fields));
        }
    }
    json!({
        "v": 1,
        "source": "session",
        "space_id": space_id,
        "generation": record.generation,
        "seq": record.seq,
        "kind": record.kind,
        "at": record.at,
        "actor": Value::Object(actor),
        "refs": Value::Object(refs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(response: Response) -> (StatusCode, HeaderMap, Value) {
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        (
            parts.status,
            parts.headers,
            serde_json::from_slice(&bytes).expect("json body"),
        )
    }

    #[test]
    fn last_event_id_parses_only_generation_dot_seq_digits() {
        assert_eq!(parse_cursor("1.0"), Some((1, 0)));
        assert_eq!(parse_cursor("12.3456"), Some((12, 3456)));
        assert_eq!(
            parse_cursor("999999999999999999.1"),
            Some((999_999_999_999_999_999, 1))
        );
        for bad in [
            "", "1", "1.", ".1", "1.2.3", "a.1", "1.b", "-1.2", "1.-2", " 1.2", "1.2 ", "1,2",
            "1.2e3", "+1.2", "0x1.2", "1234567890123456789.1", "1.1234567890123456789",
        ] {
            assert_eq!(parse_cursor(bad), None, "{bad:?}");
        }
    }

    #[tokio::test]
    async fn upstream_statuses_map_to_the_http_vocabulary() {
        for (code, http, error) in [
            (tonic::Code::Unauthenticated, StatusCode::UNAUTHORIZED, "unauthorized"),
            (tonic::Code::PermissionDenied, StatusCode::FORBIDDEN, "space_stream_forbidden"),
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST, "invalid_request"),
            (
                tonic::Code::ResourceExhausted,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_stream_capacity",
            ),
            (tonic::Code::Unavailable, StatusCode::SERVICE_UNAVAILABLE, "space_stream_unavailable"),
            (tonic::Code::Internal, StatusCode::SERVICE_UNAVAILABLE, "space_stream_unavailable"),
            (
                tonic::Code::FailedPrecondition,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_stream_unavailable",
            ),
            (tonic::Code::NotFound, StatusCode::SERVICE_UNAVAILABLE, "space_stream_unavailable"),
        ] {
            let upstream =
                tonic::Status::new(code, "upstream text must never be echoed: secret-detail");
            let (status, headers, body) = body_json(status_response(&upstream)).await;
            assert_eq!(status, http, "{code:?}");
            assert_eq!(body["error"]["code"], error, "{code:?}");
            assert_eq!(body["error"]["details"], json!({}));
            assert!(
                !body.to_string().contains("secret-detail"),
                "{code:?} leaked the upstream text"
            );
            if code == tonic::Code::ResourceExhausted {
                assert_eq!(
                    headers.get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()),
                    Some("2")
                );
            } else {
                assert!(
                    headers.get(header::RETRY_AFTER).is_none(),
                    "{code:?} must not carry Retry-After"
                );
            }
        }
    }

    #[test]
    fn header_text_trims_and_defaults_to_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("x-space-decision-ref", " dec-1 ".parse().unwrap());
        assert_eq!(header_text(&headers, DECISION_REF_HEADER), "dec-1");
        assert_eq!(header_text(&headers, DECISION_TOKEN_HEADER), "");
        assert_eq!(DECISION_REF_HEADER, "x-space-decision-ref");
        assert_eq!(DECISION_TOKEN_HEADER, "x-space-decision-token");
    }

    #[test]
    fn cursor_ids_are_generation_dot_seq() {
        assert_eq!(cursor_id(3, 41), "3.41");
    }

    #[test]
    fn a_space_event_carries_ids_and_closed_words_only() {
        let record = SpaceEventRecord {
            seq: 7,
            generation: 2,
            kind: "message.appended".to_owned(),
            at: "2027-01-15T08:00:00.000Z".to_owned(),
            actor_type: "user".to_owned(),
            actor_id: "member-2".to_owned(),
            thread_id: "thread-1".to_owned(),
            message_id: "msg-1".to_owned(),
            message_sequence: 12,
            role: "assistant".to_owned(),
            ..Default::default()
        };
        let value = space_event_json(&record, "space-room");
        assert_eq!(
            value,
            json!({
                "v": 1,
                "source": "session",
                "space_id": "space-room",
                "generation": 2,
                "seq": 7,
                "kind": "message.appended",
                "at": "2027-01-15T08:00:00.000Z",
                "actor": {"type": "user", "id": "member-2"},
                "refs": {
                    "thread_id": "thread-1",
                    "message_id": "msg-1",
                    "role": "assistant",
                    "message_sequence": 12
                }
            })
        );
    }

    #[test]
    fn empty_fields_are_omitted_and_a_stub_carries_no_refs() {
        let run = SpaceEventRecord {
            kind: "run.state_changed".to_owned(),
            actor_type: "system".to_owned(),
            run_id: "run-1".to_owned(),
            state: "running".to_owned(),
            prev_state: "queued".to_owned(),
            fields: vec!["title".to_owned()],
            ..Default::default()
        };
        let value = space_event_json(&run, "space-room");
        assert_eq!(value["actor"], json!({"type": "system"}), "no actor id when none");
        assert_eq!(
            value["refs"],
            json!({"run_id": "run-1", "state": "running", "prev": "queued", "fields": ["title"]})
        );
        for stub_kind in ["hidden", "redacted"] {
            let stub = SpaceEventRecord {
                kind: stub_kind.to_owned(),
                actor_type: "system".to_owned(),
                thread_id: "must-not-appear".to_owned(),
                run_id: "must-not-appear".to_owned(),
                message_sequence: 9,
                ..Default::default()
            };
            assert_eq!(
                space_event_json(&stub, "s")["refs"],
                json!({}),
                "{stub_kind} carries no refs"
            );
        }
    }

    #[test]
    fn every_upstream_frame_becomes_an_sse_event_and_an_empty_frame_is_dropped() {
        use mp_contracts::model_plane::v1::{
            SpaceStreamClosing, SpaceStreamConnected, SpaceStreamCursorTooOld,
            SpaceStreamHeartbeat,
        };
        let frames = [
            Frame::Connected(SpaceStreamConnected {
                mode: "live".to_owned(),
                ..Default::default()
            }),
            Frame::Connected(SpaceStreamConnected {
                mode: "poll_only".to_owned(),
                ..Default::default()
            }),
            Frame::Event(SpaceEventRecord {
                kind: "thread.created".to_owned(),
                ..Default::default()
            }),
            Frame::Heartbeat(SpaceStreamHeartbeat {
                head: 1,
                generation: 1,
                cursor: 1,
            }),
            Frame::CursorTooOld(SpaceStreamCursorTooOld {
                reason: "retention".to_owned(),
                head: 9,
                generation: 1,
            }),
            Frame::Closing(SpaceStreamClosing {
                code: "lease_expired".to_owned(),
                reconnect: true,
                ..Default::default()
            }),
        ];
        for frame in frames {
            assert!(frame_event(&SpaceEventFrame { frame: Some(frame) }, "space-room").is_some());
        }
        assert!(frame_event(&SpaceEventFrame { frame: None }, "space-room").is_none());
    }
}
