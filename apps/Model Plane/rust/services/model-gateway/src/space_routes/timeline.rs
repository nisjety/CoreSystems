//! `GET /v1/spaces/:space_id/timeline` (build plan WS15-S6): one window of the room's posts.
//!
//! A thin relay of Session Core's `SpaceRoomReadService.ListSpaceTimeline`. Authority is Session
//! Core's: this route forwards the caller's verified bearer plus the Control read decision
//! (`space_read_decision_ref` / `space_read_decision_token`, the pair `list_threads` and the
//! artifacts route take) and holds none of its own. User principals only. Summaries only: no
//! message content crosses this route beyond the compacted title and preview Session Core already
//! returns for the listing. Request URLs are never logged by this handler, and refusals carry fixed
//! words, never the upstream text. The cursors are plain message sequences here; the Frontend
//! gateway wraps them into opaque cursors that carry no authority.
//!
//! Errors, as `{error:{code,message,details}}`: flag off or Session Core's switch off 404
//! `space_timeline_disabled`, `UNAUTHENTICATED` 401 `unauthorized`, `PERMISSION_DENIED` 403
//! `space_read_not_authorized`, `INVALID_ARGUMENT` or a malformed query 400 `invalid_request`,
//! `FAILED_PRECONDITION summary_backfill_pending` 503 `summary_backfill_pending` (the gateway serves
//! the legacy listing meanwhile), anything else 503 `space_timeline_unavailable`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{ListSpaceTimelineRequest, ThreadSummary};
use mp_contracts::space_timeline::{BACKFILL_PENDING_MESSAGE, MAX_REFRESH_IDS};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{authenticated_session_request, timestamp_to_rfc3339};
use crate::state::AppState;

const MAX_SPACE_ID_BYTES: usize = 128;
const MAX_THREAD_ID_BYTES: usize = 128;

/// The Space timeline route.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/spaces/:space_id/timeline", get(list_space_timeline))
}

#[derive(Debug, Default, Deserialize)]
struct TimelineQuery {
    limit: Option<u32>,
    /// Page of posts older than this newest-message sequence.
    before_seq: Option<u64>,
    /// Page of posts newer than this newest-message sequence.
    after_seq: Option<u64>,
    /// Comma-separated thread ids to re-read (at most 30).
    refresh: Option<String>,
    /// `1` or `true`: also return the pinned posts (first page only).
    include_pinned: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response()
}

fn invalid(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

/// The HTTP form of an upstream refusal. Messages are fixed words, never the upstream text.
fn status_response(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "reading this Space's timeline is not permitted",
        ),
        tonic::Code::InvalidArgument => invalid("the Space timeline request is invalid"),
        tonic::Code::Unimplemented => disabled(),
        tonic::Code::FailedPrecondition if status.message() == BACKFILL_PENDING_MESSAGE => {
            error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                BACKFILL_PENDING_MESSAGE,
                "the Space timeline index is still being built",
            )
        }
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_timeline_unavailable",
            "the Space timeline is unavailable",
        ),
    }
}

fn disabled() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "space_timeline_disabled",
        "the Space timeline is not enabled",
    )
}

/// The same keys the thread listing uses, so one reader parses both. `session_key` is always empty
/// here: the internal key never leaves Session Core on this path.
fn summary_json(thread: &ThreadSummary) -> Value {
    let mut item = json!({
        "thread_id": thread.thread_id,
        "session_key": "",
        "title": thread.title,
        "preview": thread.preview,
        "created_at": timestamp_to_rfc3339(thread.created_at.clone()),
        "updated_at": timestamp_to_rfc3339(thread.updated_at.clone()),
        "pinned": thread.pinned,
        "space_id": thread.space_id,
        "latest_run_id": thread.latest_run_id,
        "latest_run_status": thread.latest_run_status,
        "latest_run_updated_at": thread
            .latest_run_updated_at
            .clone()
            .map(|value| timestamp_to_rfc3339(Some(value))),
        "origin": thread.origin,
        "persona_enabled": thread.persona_enabled,
        "starred": false,
        "unread": false,
        "last_message_seq": thread.last_message_seq,
        "turn_count": thread.turn_count,
    });
    if !thread.owner_subject_id.is_empty() {
        item["owner_subject_id"] = json!(thread.owner_subject_id);
    }
    if thread.reply_count > 0 {
        item["reply_count"] = json!(thread.reply_count);
    }
    if !thread.last_author_subject_id.is_empty() {
        item["last_author_subject_id"] = json!(thread.last_author_subject_id);
    }
    if !thread.recent_author_subject_ids.is_empty() {
        item["recent_author_subject_ids"] = json!(thread.recent_author_subject_ids);
    }
    if !thread.last_role.is_empty() {
        item["last_role"] = json!(thread.last_role);
    }
    item
}

fn flag_on(raw: Option<&str>) -> bool {
    matches!(raw.map(str::trim), Some("1" | "true"))
}

/// The ids of a `refresh` parameter: at most 30, each a bounded id.
fn parse_refresh(raw: Option<&str>) -> Result<Vec<String>, &'static str> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(Vec::new());
    };
    let ids: Vec<String> = raw.split(',').map(|id| id.trim().to_owned()).collect();
    if ids.len() > MAX_REFRESH_IDS {
        return Err("at most 30 refresh ids");
    }
    if ids.iter().any(|id| {
        id.is_empty()
            || id.len() > MAX_THREAD_ID_BYTES
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    }) {
        return Err("refresh contains an invalid thread id");
    }
    Ok(ids)
}

async fn list_space_timeline(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    Query(query): Query<TimelineQuery>,
) -> Response {
    // 1. Rollout flag. A rollout control only.
    if !state.space_timeline_enabled {
        return disabled();
    }
    // 2. A user principal and a bounded Space id.
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "a user principal is required",
        );
    }
    let space_id = space_id.trim().to_owned();
    if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
        return invalid("space_id is invalid");
    }
    // 3. Shape and bounds; Session Core restates them.
    if query.before_seq.unwrap_or(0) > 0 && query.after_seq.unwrap_or(0) > 0 {
        return invalid("before_seq and after_seq cannot be combined");
    }
    let refresh = match parse_refresh(query.refresh.as_deref()) {
        Ok(ids) => ids,
        Err(message) => return invalid(message),
    };
    let request = match authenticated_session_request(
        ListSpaceTimelineRequest {
            org_id: claims.org_id.clone(),
            space_id,
            space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
            space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
            before_seq: query.before_seq.unwrap_or(0),
            after_seq: query.after_seq.unwrap_or(0),
            refresh_thread_ids: refresh,
            include_pinned: flag_on(query.include_pinned.as_deref()),
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    let response = match state
        .space_room_read_client
        .clone()
        .list_space_timeline(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => return status_response(&status),
    };
    Json(json!({
        "posts": response.posts.iter().map(summary_json).collect::<Vec<_>>(),
        "pinned": response.pinned.iter().map(summary_json).collect::<Vec<_>>(),
        "refreshed": response.refreshed.iter().map(summary_json).collect::<Vec<_>>(),
        "has_more": response.has_more,
        "oldest_seq": response.oldest_seq,
        "newest_seq": response.newest_seq,
        "total_posts": response.total_posts,
    }))
    .into_response()
}
