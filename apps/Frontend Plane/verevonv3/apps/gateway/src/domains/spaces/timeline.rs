//! Space timeline windows: the BFF half of WS15-S7.
//!
//! `GET /api/v1/spaces/{space_ref}/timeline` (R1) composes one window of the room's posts from
//! model-gateway's `/v1/spaces/{id}/timeline` (Session Core's thread activity summary), the
//! caller's Control membership, a fresh Control shared-read decision and the Application read
//! marker. `parse_transcript_window` is the additive tail window of the existing transcript route
//! (R2). The timeline entries of the Space context (R5) are written in `space_context`.
//!
//! Authority is never cached or carried by a cursor. Each page re-checks the Space lifecycle,
//! the caller's live Control membership and mints its own read decision, then Session Core
//! verifies that decision and applies the audience ceiling. The cursor is `base64url("v1|" +
//! sequence)` and only says where to continue; a forged one can at worst name a position inside
//! the caller's own, freshly authorized window. Nothing here is stored: the route holds no
//! durable Space state and logs no URL, title or preview text.
//!
//! Legacy fallback: when Session Core answers `summary_backfill_pending`, or the Model Plane
//! side of the feature is off, the response is the legacy thread listing with
//! `data.page = {windowed:false, has_more:false}`, so the SPA store treats it as one complete
//! page. With this gateway's `space_timeline_api` flag off the route does not exist (404
//! `space_timeline_disabled`), which the SPA reads as "use the legacy listing".
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_cursor` / `invalid_limit` /
//! `invalid_refresh` / `invalid_space`, 403 `active_membership_required` /
//! `space_read_not_authorized`, 404 `space_not_found` / `space_timeline_disabled`, 502
//! `invalid_model_gateway_response`, 503 `space_lifecycle_unavailable` /
//! `delegated_auth_unavailable` / `space_timeline_unavailable`.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    control_membership_url, convex_gateway_call, denial, list_space_threads, public_space,
    shared_thread_read_decision_for_typed, space_lifecycle_by_ref, space_read_marker,
    thread_items_match_space, SpaceThreadsQuery,
};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
};

type Reply = (StatusCode, Json<Value>);

/// Posts per page when the caller names none, and the ceiling (Session Core restates both).
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
/// At most this many thread ids to re-read in one request.
const MAX_REFRESH_IDS: usize = 30;
const MAX_ID_LEN: usize = 128;
/// Tail turns the SPA shows of a long post before "N earlier turns" (`limits.tail_window_turns`, R5).
pub(super) const TAIL_WINDOW_TURNS: u32 = 12;
/// The most turns one tail request may ask for.
const MAX_TAIL_TURNS: u32 = 50;
/// The most system rows one page asks Control for (WS15-S11); Control restates the ceiling.
const MAX_SYSTEM_ENTRIES: u32 = 100;
const CURSOR_PREFIX: &str = "v1|";
const BACKFILL_PENDING: &str = "summary_backfill_pending";
const UPSTREAM_DISABLED: &str = "space_timeline_disabled";

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/spaces/{space_ref}/timeline", get(space_timeline))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        UPSTREAM_DISABLED,
        "The Space timeline is not enabled.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// An opaque position: `base64url("v1|" + sequence)`, optionally `"v1|" + sequence + "|" +
/// unix-ms` where the time is the activity time of the post at that position (WS15-S11: it lets
/// the next page cut the room's system rows to exactly the interval it covers). Carries no
/// authority.
fn encode_cursor(sequence: u64, at_ms: Option<i64>) -> Value {
    if sequence == 0 {
        return Value::Null;
    }
    let raw = match at_ms {
        Some(ms) => format!("{CURSOR_PREFIX}{sequence}|{ms}"),
        None => format!("{CURSOR_PREFIX}{sequence}"),
    };
    Value::String(URL_SAFE_NO_PAD.encode(raw))
}

fn decode_cursor(raw: &str) -> Option<(u64, Option<i64>)> {
    let bytes = URL_SAFE_NO_PAD.decode(raw.trim()).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let rest = text.strip_prefix(CURSOR_PREFIX)?;
    let (sequence, at_ms) = match rest.split_once('|') {
        Some((sequence, ms)) => (sequence, Some(ms.parse::<i64>().ok()?)),
        None => (rest, None),
    };
    let sequence: u64 = sequence.parse().ok()?;
    (sequence > 0).then_some((sequence, at_ms))
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TimelineQuery {
    limit: Option<String>,
    before: Option<String>,
    after: Option<String>,
    refresh: Option<String>,
}

/// A validated request: the window size, the one cursor, and the ids to re-read.
struct TimelineWindow {
    limit: u32,
    before_seq: u64,
    after_seq: u64,
    /// Activity time (unix ms) carried by the cursor, for the system-row interval (WS15-S11).
    before_ms: Option<i64>,
    after_ms: Option<i64>,
    refresh: Vec<String>,
}

impl TimelineWindow {
    /// First page: no cursor. Only it carries the membership fact, pinned shelf, read marker and
    /// total.
    fn is_first_page(&self) -> bool {
        self.before_seq == 0 && self.after_seq == 0
    }
}

fn parse_window(query: &TimelineQuery) -> Result<TimelineWindow, Reply> {
    let limit = match query.limit.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
            _ => {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_limit",
                    "limit must be a whole number from 1 to 100.",
                ))
            }
        },
    };
    let cursor = |raw: &Option<String>| -> Result<(u64, Option<i64>), Reply> {
        match raw.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            None => Ok((0, None)),
            Some(raw) => decode_cursor(raw).ok_or_else(|| {
                refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_cursor",
                    "The timeline cursor is not valid.",
                )
            }),
        }
    };
    let (before_seq, before_ms) = cursor(&query.before)?;
    let (after_seq, after_ms) = cursor(&query.after)?;
    if before_seq > 0 && after_seq > 0 {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "before and after cannot be combined.",
        ));
    }
    let mut refresh = Vec::new();
    if let Some(raw) = query.refresh.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        for id in raw.split(',').map(str::trim) {
            if !valid_id(id) {
                return Err(refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_refresh",
                    "refresh must list valid thread ids.",
                ));
            }
            refresh.push(id.to_owned());
        }
        if refresh.len() > MAX_REFRESH_IDS {
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                "invalid_refresh",
                "refresh lists too many thread ids.",
            ));
        }
    }
    Ok(TimelineWindow {
        limit,
        before_seq,
        after_seq,
        before_ms,
        after_ms,
        refresh,
    })
}

/// The error code of a model-gateway failure body, when it has one.
fn upstream_code(body: &Value) -> Option<&str> {
    body.get("error")?.get("code")?.as_str()
}

/// R1.
async fn space_timeline(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(query): Query<TimelineQuery>,
) -> Reply {
    // 1. Rollout flag. A rollout control only.
    if !state.space_features.timeline_api {
        return disabled();
    }
    // 2. Organization, Space reference and request shape: all refused before any upstream call.
    let org_id = authorized_org_id(&state, &user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() {
        return refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to read the Space timeline.",
        );
    }
    if space_ref.is_empty() {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let window = match parse_window(&query) {
        Ok(window) => window,
        Err(response) => return response,
    };
    // 3. Lifecycle: a suspended or deleting room serves nothing on a membership row alone.
    let space = match space_lifecycle_by_ref(&state, &user, &org_id, &space_ref).await {
        Ok(Some(space)) => space,
        Ok(None) => {
            return refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            )
        }
        Err(()) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            )
        }
    };
    // 4. The caller's live Control membership, on every page.
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let membership_url = control_membership_url(&state, &space_ref, true);
    let (membership_status, Json(membership)) = proxy_json(
        &state,
        Method::GET,
        &membership_url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    if !membership_status.is_success() {
        return denial::typed_response(&state, membership_status, membership, denial::OP_READ);
    }
    let session_token =
        match crate::domains::chat::shared::required_session_token(&state, &user, &headers).await {
            Ok(token) => token,
            Err(reason) => return crate::domains::chat::shared::delegated_auth_unavailable(reason),
        };
    // 5. A fresh Control read decision for THIS page. `None` is a refusal here: a window cannot be
    //    built from the owner-bound listing.
    let (read, read_denial) = match shared_thread_read_decision_for_typed(
        &state, &user, &org_id, &space_ref, "",
    )
    .await
    {
        Ok(decision) => decision.split(),
        Err(response) => return response,
    };
    let Some((decision_ref, decision_token)) = read else {
        let message = "Reading this Space's shared conversation is not authorized.";
        return match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            ),
            None => refuse(
                StatusCode::FORBIDDEN,
                "space_read_not_authorized",
                message,
            ),
        };
    };
    // 6. model-gateway. The decision rides the query like the two existing shared reads, so the
    //    URL is never logged here.
    let mut url = format!(
        "{}/v1/spaces/{}/timeline?limit={}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(&space_ref),
        window.limit,
        urlencoding::encode(&decision_ref),
        urlencoding::encode(&decision_token),
    );
    if window.before_seq > 0 {
        url.push_str(&format!("&before_seq={}", window.before_seq));
    }
    if window.after_seq > 0 {
        url.push_str(&format!("&after_seq={}", window.after_seq));
    }
    if !window.refresh.is_empty() {
        url.push_str(&format!(
            "&refresh={}",
            urlencoding::encode(&window.refresh.join(","))
        ));
    }
    if window.is_first_page() {
        url.push_str("&include_pinned=1");
    }
    let model_token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        model_token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return match upstream_code(&payload) {
            // Session Core is still building the summary, or the Model Plane side is off: serve
            // the legacy listing instead of an error.
            Some(BACKFILL_PENDING) | Some(UPSTREAM_DISABLED) => {
                legacy_fallback(state, user, headers, space_ref).await
            }
            Some(_) => (status, Json(payload)),
            None => refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_timeline_unavailable",
                "The Space timeline is unavailable.",
            ),
        };
    }
    // 7. The answer must be the shape and the Space asked for.
    let list = |key: &str| payload.get(key).cloned().unwrap_or_else(|| json!([]));
    let (posts, pinned, refreshed) = (list("posts"), list("pinned"), list("refreshed"));
    if !(posts.is_array() && pinned.is_array() && refreshed.is_array()) {
        return refuse(
            StatusCode::BAD_GATEWAY,
            "invalid_model_gateway_response",
            "Model Gateway returned an invalid timeline.",
        );
    }
    for items in [&posts, &pinned, &refreshed] {
        if !thread_items_match_space(items, &space_ref) {
            return refuse(
                StatusCode::BAD_GATEWAY,
                "invalid_model_gateway_response",
                "Model Gateway returned a thread outside the selected Space.",
            );
        }
    }
    // Session Core returns newest activity first; the room reads oldest first.
    let mut posts = posts.as_array().cloned().unwrap_or_default();
    posts.reverse();
    let has_more = payload
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let oldest_seq = payload.get("oldest_seq").and_then(Value::as_u64).unwrap_or(0);
    let newest_seq = payload.get("newest_seq").and_then(Value::as_u64).unwrap_or(0);
    let first_page = window.is_first_page();
    let after_direction = window.after_seq > 0;
    // WS15-S11: the activity-time span of this window, for the cursors and the system-row interval.
    let (oldest_ms, newest_ms) = post_time_span(&posts);
    let page = json!({
        "windowed": true,
        "limit": window.limit,
        "has_more": has_more,
        // Older direction only: where "Load earlier" continues.
        "next_before": if has_more && !after_direction { encode_cursor(oldest_seq, oldest_ms) } else { Value::Null },
        // Newer direction only: where an `after` refresh continues when the window was cut.
        "next_after": if has_more && after_direction { encode_cursor(newest_seq, newest_ms) } else { Value::Null },
        "newest": encode_cursor(newest_seq, newest_ms),
        "total_posts": if first_page {
            payload.get("total_posts").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        },
    });
    let mut data = json!({
        "space": public_space(&space),
        "threads": posts,
        "pinned": pinned,
        "system_entries": [],
        "page": page,
        "refreshed": refreshed,
    });
    // 7b. WS15-S11: the room's system rows for the interval this page covers. Control failing is a
    //     named gap, never a failed window.
    //     WS15-S12: Application's room notices (agent and room changes) are merged into the same
    //     rows; Application failing is its own named gap and keeps Control's rows.
    let mut system_gaps: Vec<Value> = Vec::new();
    if state.space_features.system_entries_api {
        let interval = system_interval(&window, has_more, oldest_ms, newest_ms);
        let control = system_entries(&state, &org_id, &space_ref, &actor, interval).await;
        let notices = room_notices(&state, &user, &org_id, &space_ref, interval).await;
        if control.is_none() {
            system_gaps.push(json!({
                "section": "system_entries",
                "code": "system_entries_unavailable",
                "reason": "Control could not return the room's membership changes.",
            }));
        }
        if notices.is_none() {
            system_gaps.push(json!({
                "section": "system_entries",
                "code": "room_notices_unavailable",
                "reason": "Application Plane could not return the room's agent and room changes.",
            }));
        }
        if control.is_some() || notices.is_some() {
            let (entries, truncated) = merge_system_rows(control, notices);
            data["system_entries"] = Value::Array(entries);
            data["system_truncated"] = Value::Bool(truncated);
        }
    }
    // 8. First page only: the membership fact, the room's own pins, the agent-work offer, where the
    //    member last caught up. Application answering slowly or not at all is a named gap, never a
    //    failed window.
    if first_page {
        let mut unavailable: Vec<Value> = Vec::new();
        let read_marker = match space_read_marker(&state, &user, &org_id, &space_ref).await {
            Some(marker) => marker,
            None => {
                unavailable.push(json!({
                    "section": "read_marker",
                    "code": "read_marker_unavailable",
                    "reason": "Application Plane could not return where you last caught up.",
                }));
                Value::Null
            }
        };
        let pins = if state.space_features.room_pins {
            match super::pins::list_pins(&state, &user, &org_id, &space_ref).await {
                Some(pins) => pins,
                None => {
                    unavailable.push(json!({
                        "section": "pins",
                        "code": "pins_unavailable",
                        "reason": "Application Plane could not return the room's pins.",
                    }));
                    json!([])
                }
            }
        } else {
            json!([])
        };
        data["membership"] = crate::envelope::unwrap_data(&membership);
        data["pins"] = pins;
        data["agent_work"] = super::agent_work_offer(&state, &user).await;
        data["shared_read"] = json!({ "available": true });
        data["read_marker"] = read_marker;
        unavailable.append(&mut system_gaps);
        data["unavailable"] = Value::Array(unavailable);
    }
    if !system_gaps.is_empty() {
        data["unavailable"] = Value::Array(system_gaps);
    }
    (StatusCode::OK, Json(json!({ "data": data })))
}

/// The oldest and newest activity time (unix ms) among the posts of a window, from each post's
/// `updated_at`. `None` when no post carries a parseable time.
fn post_time_span(posts: &[Value]) -> (Option<i64>, Option<i64>) {
    let mut times = posts.iter().filter_map(|post| {
        let raw = post.get("updated_at")?.as_str()?;
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|at| at.timestamp_millis())
    });
    let Some(first) = times.next() else {
        return (None, None);
    };
    let (lo, hi) = times.fold((first, first), |(lo, hi), ms| (lo.min(ms), hi.max(ms)));
    (Some(lo), Some(hi))
}

/// The interval a page's system rows cover, `[lo, hi)` in unix ms; `None` is unbounded.
///
/// Older pages tile by time without a merged cursor: a page covers from its own oldest post
/// (unbounded when the window reached the start of the room) up to the oldest post of the page
/// before it, which the `before` cursor carries. The first page is open at the top, so rows newer
/// than the newest post still show. A refresh (`after`) covers from the cursor's post time upward;
/// a cut refresh window stops at its own newest post and the next `after` page continues.
fn system_interval(
    window: &TimelineWindow,
    has_more: bool,
    oldest_ms: Option<i64>,
    newest_ms: Option<i64>,
) -> (Option<i64>, Option<i64>) {
    if window.after_seq > 0 {
        let hi = if has_more { newest_ms.map(|ms| ms + 1) } else { None };
        return (window.after_ms, hi);
    }
    let lo = if has_more { oldest_ms } else { None };
    (lo, window.before_ms)
}

fn rfc3339_from_ms(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// The closed set of ledger words the room shows; anything else is dropped.
const SYSTEM_ACTIONS: [&str; 9] = [
    "member_added",
    "member_joined",
    "role_changed",
    "member_removed",
    "member_left",
    "archived",
    "unarchived",
    "visibility_changed",
    "settings_changed",
];

/// Control's membership ledger rows for the interval, as timeline rows newest first, plus whether
/// Control cut the list. `None` when Control could not answer. Rows hold ids and closed words
/// only; the BFF stores nothing and logs nothing from them.
async fn system_entries(
    state: &AppState,
    org_id: &str,
    space_ref: &str,
    actor: &ActionActor,
    interval: (Option<i64>, Option<i64>),
) -> Option<(Vec<Value>, bool)> {
    let mut url = format!(
        "{}/api/v1/internal/spaces/{}/membership-events?limit={}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        MAX_SYSTEM_ENTRIES
    );
    if let Some(lo) = interval.0.and_then(rfc3339_from_ms) {
        url.push_str(&format!("&after={}", urlencoding::encode(&lo)));
    }
    if let Some(hi) = interval.1.and_then(rfc3339_from_ms) {
        url.push_str(&format!("&before={}", urlencoding::encode(&hi)));
    }
    let (status, Json(body)) =
        proxy_json(state, Method::GET, &url, None, Some(org_id), Some(actor), None).await;
    if !status.is_success() {
        return None;
    }
    let data = crate::envelope::unwrap_data(&body);
    let events = data.get("events")?.as_array()?;
    let truncated = data
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut entries = Vec::with_capacity(events.len());
    for event in events {
        let field = |name: &str| event.get(name).and_then(Value::as_str).unwrap_or("");
        let action = field("action");
        if field("event_id").is_empty() || !SYSTEM_ACTIONS.contains(&action) {
            continue;
        }
        entries.push(json!({
            "id": field("event_id"),
            "kind": "membership",
            "action": action,
            "space_seq": event.get("space_seq").cloned().unwrap_or(Value::Null),
            "actor_kind": field("actor_kind"),
            "actor_id": field("actor_id"),
            "subject_type": field("subject_type"),
            "subject_id": field("subject_id"),
            "from_role": field("from_role"),
            "to_role": field("to_role"),
            "created_at": field("created_at"),
        }));
    }
    Some((entries, truncated))
}

/// The closed set of Application notice words the room shows; anything else is dropped.
const NOTICE_KINDS: [&str; 6] = [
    "agent_bound",
    "agent_paused",
    "agent_resumed",
    "agent_revoked",
    "room_renamed",
    "topic_changed",
];

/// Application's room notices for the interval, as timeline rows newest first, plus whether
/// Application cut the list. `None` when Application could not answer. Rows hold ids and closed
/// words only (no names); the BFF stores nothing and logs nothing from them.
async fn room_notices(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    interval: (Option<i64>, Option<i64>),
) -> Option<(Vec<Value>, bool)> {
    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "limit": MAX_SYSTEM_ENTRIES,
    });
    if let Some(lo) = interval.0 {
        args["after"] = json!(lo);
    }
    if let Some(hi) = interval.1 {
        args["before"] = json!(hi);
    }
    let result = convex_gateway_call(state, "query", "spaceRoomNotices:roomNoticesForGateway", args)
        .await
        .ok()?;
    let value = result.get("value").unwrap_or(&result);
    let rows = value.get("notices")?.as_array()?;
    let truncated = value
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let text = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or("");
        let kind = text("kind");
        let id = text("noticeId");
        let created_at = row
            .get("createdAt")
            .and_then(Value::as_i64)
            .and_then(rfc3339_from_ms);
        let (false, true, Some(created_at)) = (id.is_empty(), NOTICE_KINDS.contains(&kind), created_at)
        else {
            continue;
        };
        entries.push(json!({
            "id": id,
            "kind": "notice",
            "action": kind,
            "actor_id": text("actorExternalAuthId"),
            "subject_id": text("subjectId"),
            "created_at": created_at,
        }));
    }
    Some((entries, truncated))
}

/// Control rows and Application notices as one list, newest first by `(created_at, id)`, cut to
/// `MAX_SYSTEM_ENTRIES`. Truncated when either source was cut or the merge itself dropped rows.
/// The timestamps are RFC 3339 UTC with the same precision from Control and from here, so the
/// string order is the time order.
fn merge_system_rows(
    control: Option<(Vec<Value>, bool)>,
    notices: Option<(Vec<Value>, bool)>,
) -> (Vec<Value>, bool) {
    let mut truncated = false;
    let mut rows: Vec<Value> = Vec::new();
    for (mut part, cut) in [control, notices].into_iter().flatten() {
        truncated |= cut;
        rows.append(&mut part);
    }
    let key = |row: &Value| {
        (
            row.get("created_at")
                .and_then(Value::as_str)
                .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
                .map(|at| at.timestamp_millis())
                .unwrap_or(0),
            row.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
        )
    };
    rows.sort_by(|a, b| key(b).cmp(&key(a)));
    if rows.len() > MAX_SYSTEM_ENTRIES as usize {
        rows.truncate(MAX_SYSTEM_ENTRIES as usize);
        truncated = true;
    }
    (rows, truncated)
}

/// The legacy thread listing, shaped as one complete page. It re-runs the legacy handler's own
/// checks, so nothing here widens what the caller may read.
async fn legacy_fallback(
    state: AppState,
    user: AuthenticatedUser,
    headers: HeaderMap,
    space_ref: String,
) -> Reply {
    let (status, Json(mut body)) = list_space_threads(
        State(state),
        Extension(user),
        headers,
        Path(space_ref),
        Query(SpaceThreadsQuery { limit: None }),
    )
    .await;
    if status.is_success() {
        if let Some(data) = body.get_mut("data").and_then(Value::as_object_mut) {
            data.insert(
                "page".to_owned(),
                json!({ "windowed": false, "has_more": false }),
            );
        }
    }
    (status, Json(body))
}

// ---- R2: the additive tail window of the transcript route ----------------------------------

/// `?tail=12&before=<message_id>&include_first=1` on the existing transcript route.
#[derive(Debug, Default, Deserialize)]
pub(super) struct TranscriptWindowQuery {
    tail: Option<String>,
    before: Option<String>,
    include_first: Option<String>,
}

/// A validated tail window request.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TranscriptWindow {
    tail: u32,
    before: Option<String>,
    include_first: bool,
}

impl TranscriptWindow {
    /// The model-gateway query parameters that ask for this window.
    pub(super) fn query_suffix(&self) -> String {
        let mut out = format!("&limit={}", self.tail);
        if let Some(before) = &self.before {
            out.push_str(&format!("&before={}", urlencoding::encode(before)));
        } else if self.include_first {
            out.push_str("&include_first=1");
        }
        out
    }
}

fn invalid_window(message: &'static str) -> Reply {
    refuse(StatusCode::BAD_REQUEST, "invalid_window", message)
}

/// Window parameters are honoured only while `space_timeline_api` is on. With the flag off, or no
/// window parameter at all, the transcript is the whole conversation exactly as before.
pub(super) fn parse_transcript_window(
    enabled: bool,
    query: &TranscriptWindowQuery,
) -> Result<Option<TranscriptWindow>, Reply> {
    let given = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
    };
    if !enabled || !(given(&query.tail) || given(&query.before) || given(&query.include_first)) {
        return Ok(None);
    }
    let Some(raw_tail) = query.tail.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return Err(invalid_window("before and include_first need tail."));
    };
    let tail = match raw_tail.parse::<u32>() {
        Ok(tail) if (1..=MAX_TAIL_TURNS).contains(&tail) => tail,
        _ => return Err(invalid_window("tail must be a whole number from 1 to 50.")),
    };
    let before = match query.before.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(before) if valid_id(before) => Some(before.to_owned()),
        Some(_) => return Err(invalid_window("before must be a message id.")),
    };
    let include_first = match query
        .include_first
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        None | Some("0" | "false") => false,
        Some("1" | "true") => true,
        Some(_) => return Err(invalid_window("include_first must be 1 or 0.")),
    };
    Ok(Some(TranscriptWindow {
        tail,
        before,
        include_first,
    }))
}

/// `data.transcript.window` from the model-gateway body of a tail read.
pub(super) fn transcript_window_json(payload: &Value) -> Value {
    let omitted_before = payload
        .get("omitted_before")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let has_more = payload
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    json!({
        "omitted_before": omitted_before,
        "has_more_before": has_more || omitted_before > 0,
    })
}
