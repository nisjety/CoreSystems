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

#[cfg(test)]
mod tests {
    use super::{
        decode_cursor, encode_cursor, parse_transcript_window, parse_window, space_timeline,
        TimelineQuery, TranscriptWindowQuery, MAX_REFRESH_IDS, TAIL_WINDOW_TURNS,
    };
    use crate::config::AppState;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
    use axum::{
        extract::{Extension, Path, Query, State},
        http::{HeaderMap, StatusCode},
        Json,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::{json, Value};
    use wiremock::matchers::{body_string_contains, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "space-1";

    fn member() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "timeline-user".to_owned(),
            user_email: "timeline@example.com".to_owned(),
            user_name: "Timeline".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn cursor(raw: &str) -> String {
        URL_SAFE_NO_PAD.encode(raw)
    }

    fn query(
        limit: Option<&str>,
        before: Option<&str>,
        after: Option<&str>,
        refresh: Option<&str>,
    ) -> TimelineQuery {
        TimelineQuery {
            limit: limit.map(str::to_owned),
            before: before.map(str::to_owned),
            after: after.map(str::to_owned),
            refresh: refresh.map(str::to_owned),
        }
    }

    fn code_of(result: Result<impl Sized, (StatusCode, Json<Value>)>) -> (u16, String) {
        match result {
            Ok(_) => panic!("expected a refusal"),
            Err((status, Json(body))) => (
                status.as_u16(),
                body["error"]["code"].as_str().unwrap_or_default().to_owned(),
            ),
        }
    }

    // ----- fixture ---------------------------------------------------------------------------

    struct Fx {
        server: MockServer,
        state: AppState,
    }

    async fn mount(server: &MockServer, method: &str, route: &str, template: ResponseTemplate) {
        Mock::given(wm_method(method))
            .and(wm_path(route))
            .respond_with(template)
            .mount(server)
            .await;
    }

    fn post(thread_id: &str, space_id: &str, updated_at: &str) -> Value {
        json!({ "thread_id": thread_id, "space_id": space_id, "updated_at": updated_at })
    }

    /// Session Core returns the newest activity first.
    fn window_payload() -> Value {
        json!({
            "posts": [
                post("t2", SPACE, "2026-11-01T10:05:00Z"),
                post("t1", SPACE, "2026-11-01T10:00:00Z"),
            ],
            "pinned": [],
            "refreshed": [],
            "has_more": true,
            "oldest_seq": 5,
            "newest_seq": 9,
            "total_posts": 42,
        })
    }

    /// `marker`: whether Application answers the read marker. Anything else Application is asked
    /// stays unmatched (404), which is a named gap for pins.
    async fn fixture(payload: ResponseTemplate, marker: bool) -> Fx {
        let server = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_string_contains("spacesForOrgForGateway"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [{ "spaceRef": SPACE, "lifecycle": "active", "name": "Room", "kind": "room" }]
            })))
            .mount(&server)
            .await;
        if marker {
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/query"))
                .and(body_string_contains("spaceReadMarkerForGateway"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "value": { "lastReadAt": 123 } })),
                )
                .mount(&server)
                .await;
        }
        mount(
            &server,
            "GET",
            &format!("/api/v1/internal/spaces/{SPACE}/membership"),
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "role": "member", "state": "active" } })),
        )
        .await;
        mount(
            &server,
            "GET",
            "/api/session-core/token",
            ResponseTemplate::new(200).set_body_json(json!({ "token": "session-test-token" })),
        )
        .await;
        mount(
            &server,
            "POST",
            "/api/v1/internal/spaces/thread-read-decision",
            ResponseTemplate::new(200).set_body_json(json!({
                "data": { "decision": { "decision_ref": "dec-1" }, "token": "dec-token" }
            })),
        )
        .await;
        mount(&server, "GET", &format!("/v1/spaces/{SPACE}/timeline"), payload).await;
        let mut state = crate::tests::test_state(false);
        let uri = server.uri();
        state.auth_core_url = uri.clone();
        state.user_core_url = uri.clone();
        state.model_gateway_url = uri.clone();
        state.org_core_url = uri.clone();
        state.application_convex_url = uri;
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.timeline_api = true;
        Fx { server, state }
    }

    async fn timeline(fx: &Fx, q: TimelineQuery) -> (u16, Value) {
        let (status, Json(body)) = space_timeline(
            State(fx.state.clone()),
            Extension(member()),
            HeaderMap::new(),
            Path(SPACE.to_owned()),
            Query(q),
        )
        .await;
        (status.as_u16(), body)
    }

    async fn requests_to(server: &MockServer, needle: &str) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().contains(needle))
            .count()
    }

    async fn timeline_url(server: &MockServer) -> String {
        server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .rev()
            .find(|request| request.url.path().ends_with("/timeline"))
            .expect("a model-gateway timeline call")
            .url
            .to_string()
    }

    // ----- cursors and request validation (pure) --------------------------------------------

    #[test]
    fn cursors_roundtrip_and_carry_only_a_position() {
        assert_eq!(decode_cursor(encode_cursor(7, None).as_str().unwrap()), Some((7, None)));
        assert_eq!(
            decode_cursor(encode_cursor(7, Some(1_700_000_000_000)).as_str().unwrap()),
            Some((7, Some(1_700_000_000_000)))
        );
        assert_eq!(encode_cursor(0, Some(5)), Value::Null);
        let raw = URL_SAFE_NO_PAD.decode(encode_cursor(7, None).as_str().unwrap()).unwrap();
        assert_eq!(raw, b"v1|7");
    }

    #[test]
    fn timeline_invalid_cursor_is_400() {
        for bad in [
            "!!!not-base64!!!".to_owned(),
            cursor("v2|5"),
            cursor("5"),
            cursor("v1|0"),
            cursor("v1|abc"),
            cursor("v1|5|notanumber"),
            cursor("v1|-3"),
        ] {
            let (status, code) = code_of(parse_window(&query(None, Some(&bad), None, None)).map(|_| ()));
            assert_eq!((status, code.as_str()), (400, "invalid_cursor"), "before {bad}");
            let (status, code) = code_of(parse_window(&query(None, None, Some(&bad), None)).map(|_| ()));
            assert_eq!((status, code.as_str()), (400, "invalid_cursor"), "after {bad}");
        }
        let both = cursor("v1|5");
        let (status, code) =
            code_of(parse_window(&query(None, Some(&both), Some(&both), None)).map(|_| ()));
        assert_eq!((status, code.as_str()), (400, "invalid_cursor"));
        // A valid single cursor and a blank one are accepted.
        let window = parse_window(&query(None, Some(&both), None, None)).ok().unwrap();
        assert_eq!(window.before_seq, 5);
        assert!(!window.is_first_page());
        assert!(parse_window(&query(None, Some("  "), None, None)).ok().unwrap().is_first_page());
    }

    #[test]
    fn timeline_invalid_limit_and_refresh_are_400() {
        for bad in ["0", "101", "abc", "-1", "1.5"] {
            let (status, code) = code_of(parse_window(&query(Some(bad), None, None, None)).map(|_| ()));
            assert_eq!((status, code.as_str()), (400, "invalid_limit"), "{bad}");
        }
        assert_eq!(parse_window(&query(None, None, None, None)).ok().unwrap().limit, 50);
        assert_eq!(parse_window(&query(Some("1"), None, None, None)).ok().unwrap().limit, 1);
        assert_eq!(parse_window(&query(Some("100"), None, None, None)).ok().unwrap().limit, 100);

        let too_many = (0..=MAX_REFRESH_IDS).map(|i| format!("t{i}")).collect::<Vec<_>>().join(",");
        let at_most = (0..MAX_REFRESH_IDS).map(|i| format!("t{i}")).collect::<Vec<_>>().join(",");
        let (status, code) =
            code_of(parse_window(&query(None, None, None, Some(&too_many))).map(|_| ()));
        assert_eq!((status, code.as_str()), (400, "invalid_refresh"));
        assert_eq!(
            parse_window(&query(None, None, None, Some(&at_most))).ok().unwrap().refresh.len(),
            MAX_REFRESH_IDS
        );
        for bad in ["a b", "t1,,t2", "t1,../x", "t/1", &"a".repeat(129)] {
            let (status, code) = code_of(parse_window(&query(None, None, None, Some(bad))).map(|_| ()));
            assert_eq!((status, code.as_str()), (400, "invalid_refresh"), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_bad_request_shape_is_refused_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        for q in [
            query(Some("0"), None, None, None),
            query(None, Some("garbage"), None, None),
            query(None, None, None, Some("a b")),
        ] {
            let (status, _) = timeline(&fx, q).await;
            assert_eq!(status, 400);
        }
        assert!(fx.server.received_requests().await.unwrap().is_empty());
    }

    // ----- flag, composition -----------------------------------------------------------------

    #[tokio::test]
    async fn timeline_flag_off_is_404_space_timeline_disabled_with_no_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        fx.state.space_features.timeline_api = false;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_timeline_disabled");
        assert!(fx.server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn timeline_first_page_composes_space_membership_marker() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        let data = &body["data"];
        assert_eq!(data["space"]["space_ref"], SPACE);
        assert_eq!(data["membership"]["role"], "member");
        assert_eq!(data["pins"], json!([]));
        assert_eq!(data["read_marker"]["last_read_at"], 123);
        assert_eq!(data["page"]["total_posts"], 42);
        assert_eq!(data["page"]["windowed"], true);
        assert_eq!(data["shared_read"]["available"], true);
        assert_eq!(data["unavailable"], json!([]));
        // The room reads oldest first.
        assert_eq!(data["threads"][0]["thread_id"], "t1");
        assert_eq!(data["threads"][1]["thread_id"], "t2");
        let url = timeline_url(&fx.server).await;
        assert!(url.contains("include_pinned=1"), "{url}");
        assert!(url.contains("limit=50"), "{url}");
        assert!(!url.contains("before_seq") && !url.contains("after_seq"), "{url}");
    }

    #[tokio::test]
    async fn a_later_page_carries_no_membership_pins_marker_or_total() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let (status, body) =
            timeline(&fx, query(Some("20"), Some(&cursor("v1|5")), None, Some("t1,t2"))).await;
        assert_eq!(status, 200);
        let data = body["data"].as_object().unwrap();
        for absent in ["membership", "pins", "read_marker", "agent_work", "shared_read", "unavailable"] {
            assert!(!data.contains_key(absent), "{absent} is first-page only");
        }
        assert!(body["data"]["page"]["total_posts"].is_null());
        let url = timeline_url(&fx.server).await;
        assert!(url.contains("before_seq=5"), "{url}");
        assert!(url.contains("limit=20"), "{url}");
        assert!(url.contains("refresh=t1%2Ct2"), "{url}");
        assert!(!url.contains("include_pinned"), "{url}");
        assert_eq!(requests_to(&fx.server, "spaceReadMarker").await, 0);
    }

    #[tokio::test]
    async fn timeline_posts_are_ascending_and_cursors_roundtrip() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let (_, body) = timeline(&fx, TimelineQuery::default()).await;
        let page = &body["data"]["page"];
        // Older direction: next_before points at the OLDEST post of this window.
        let (seq, at_ms) = decode_cursor(page["next_before"].as_str().unwrap()).unwrap();
        assert_eq!(seq, 5);
        assert_eq!(
            at_ms,
            Some(chrono::DateTime::parse_from_rfc3339("2026-11-01T10:00:00Z").unwrap().timestamp_millis())
        );
        assert!(page["next_after"].is_null());
        let (seq, at_ms) = decode_cursor(page["newest"].as_str().unwrap()).unwrap();
        assert_eq!(seq, 9);
        assert_eq!(
            at_ms,
            Some(chrono::DateTime::parse_from_rfc3339("2026-11-01T10:05:00Z").unwrap().timestamp_millis())
        );
        // Newer direction: next_after continues from the NEWEST post and next_before stays null.
        let (_, body) = timeline(&fx, query(None, None, Some(&cursor("v1|3")), None)).await;
        let page = &body["data"]["page"];
        assert!(page["next_before"].is_null());
        assert_eq!(decode_cursor(page["next_after"].as_str().unwrap()).unwrap().0, 9);
        // A window that reached the end has no continuation.
        let mut done = window_payload();
        done["has_more"] = json!(false);
        let fx = fixture(ResponseTemplate::new(200).set_body_json(done), true).await;
        let (_, body) = timeline(&fx, TimelineQuery::default()).await;
        assert!(body["data"]["page"]["next_before"].is_null());
        assert!(body["data"]["page"]["next_after"].is_null());
        assert_eq!(body["data"]["page"]["has_more"], false);
    }

    // ----- authority per page ----------------------------------------------------------------

    #[tokio::test]
    async fn timeline_cursor_carries_no_authority_each_page_mints_a_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        timeline(&fx, TimelineQuery::default()).await;
        timeline(&fx, query(None, Some(&cursor("v1|5")), None, None)).await;
        timeline(&fx, query(None, None, Some(&cursor("v1|3")), None)).await;
        assert_eq!(requests_to(&fx.server, "space-1/membership").await, 3);
        assert_eq!(requests_to(&fx.server, "thread-read-decision").await, 3);
        // The decision rides to model-gateway on every page.
        let urls: Vec<String> = fx
            .server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path().ends_with("/timeline"))
            .map(|request| request.url.to_string())
            .collect();
        assert_eq!(urls.len(), 3);
        for url in urls {
            assert!(url.contains("space_read_decision_ref=dec-1"), "{url}");
            assert!(url.contains("space_read_decision_token=dec-token"), "{url}");
        }
    }

    #[tokio::test]
    async fn a_declined_read_decision_is_403_space_read_not_authorized_and_nothing_is_read() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        fx.server.reset().await;
        // reset drops every mock: remount only what the route needs up to the decision.
        let fx = {
            let Fx { server, state } = fx;
            Mock::given(wm_method("POST"))
                .and(wm_path("/api/query"))
                .and(body_string_contains("spacesForOrgForGateway"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "value": [{ "spaceRef": SPACE, "lifecycle": "active" }]
                })))
                .mount(&server)
                .await;
            mount(
                &server,
                "GET",
                &format!("/api/v1/internal/spaces/{SPACE}/membership"),
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "role": "member" } })),
            )
            .await;
            mount(
                &server,
                "GET",
                "/api/session-core/token",
                ResponseTemplate::new(200).set_body_json(json!({ "token": "t" })),
            )
            .await;
            mount(
                &server,
                "POST",
                "/api/v1/internal/spaces/thread-read-decision",
                ResponseTemplate::new(403).set_body_json(json!({
                    "error": "No.", "code": "space_membership_required", "resolvable_by": "none"
                })),
            )
            .await;
            Fx { server, state }
        };
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_read_not_authorized");
        assert!(body["error"].get("details").map_or(true, |d| d.as_object().map_or(true, |o| o.is_empty())));
        assert_eq!(requests_to(&fx.server, "/timeline").await, 0);

        // With typed denials on, the same decline carries the reason and who can resolve it.
        let mut typed = fx;
        typed.state.space_features.denials_typed = true;
        let (status, body) = timeline(&typed, TimelineQuery::default()).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "space_read_not_authorized");
        assert_eq!(body["error"]["details"]["reason"], "space_membership_required");
        assert_eq!(body["error"]["details"]["resolvable_by"], "none");
        assert_eq!(requests_to(&typed.server, "/timeline").await, 0);
    }

    #[tokio::test]
    async fn a_non_member_gets_controls_refusal_and_a_missing_space_is_404() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": [] })))
            .mount(&server)
            .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = server.uri();
        state.auth_core_url = server.uri();
        state.model_gateway_url = server.uri();
        state.application_convex_url = server.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.timeline_api = true;
        let fx = Fx { server, state };
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("space_not_found")));
        assert_eq!(requests_to(&fx.server, "space-1/membership").await, 0);

        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        fx.server.reset().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [{ "spaceRef": SPACE, "lifecycle": "active" }]
            })))
            .mount(&fx.server)
            .await;
        mount(
            &fx.server,
            "GET",
            &format!("/api/v1/internal/spaces/{SPACE}/membership"),
            ResponseTemplate::new(403).set_body_json(json!({
                "error": { "code": "not_a_space_member", "message": "no" }
            })),
        )
        .await;
        let (status, _) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 403);
        assert_eq!(requests_to(&fx.server, "thread-read-decision").await, 0);
        assert_eq!(requests_to(&fx.server, "/timeline").await, 0);
    }

    // ----- legacy fallback and failures ------------------------------------------------------

    async fn legacy_fixture(upstream: ResponseTemplate) -> Fx {
        let fx = fixture(upstream, true).await;
        mount(
            &fx.server,
            "GET",
            "/v1/threads",
            ResponseTemplate::new(200).set_body_json(json!({
                "threads": [post("legacy-1", SPACE, "2026-11-01T09:00:00Z")]
            })),
        )
        .await;
        fx
    }

    #[tokio::test]
    async fn timeline_falls_back_to_legacy_on_backfill_pending() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (status, code) in [
            (503u16, "summary_backfill_pending"),
            (409, "summary_backfill_pending"),
            (404, "space_timeline_disabled"),
        ] {
            let fx = legacy_fixture(
                ResponseTemplate::new(status)
                    .set_body_json(json!({ "error": { "code": code, "message": "m" } })),
            )
            .await;
            let (got, body) = timeline(&fx, TimelineQuery::default()).await;
            assert_eq!(got, 200, "{code}");
            assert_eq!(body["data"]["page"], json!({ "windowed": false, "has_more": false }), "{code}");
            assert_eq!(body["data"]["threads"][0]["thread_id"], "legacy-1");
            assert_eq!(requests_to(&fx.server, "/v1/threads").await, 1);
        }
    }

    #[tokio::test]
    async fn other_upstream_failures_keep_their_typed_status_or_are_the_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = legacy_fixture(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "error": { "code": "space_read_not_authorized", "message": "m" } })),
        )
        .await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!((status, body["error"]["code"].clone()), (403, json!("space_read_not_authorized")));
        assert_eq!(requests_to(&fx.server, "/v1/threads").await, 0);

        let fx = legacy_fixture(ResponseTemplate::new(500).set_body_string("down")).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!((status, body["error"]["code"].clone()), (503, json!("space_timeline_unavailable")));
    }

    #[tokio::test]
    async fn timeline_source_failure_is_a_named_gap() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // No read-marker mock (marker=false) and pins on with no pins mock: Application answers 404.
        let mut fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), false).await;
        fx.state.space_features.room_pins = true;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200, "the window is still served");
        assert_eq!(body["data"]["threads"].as_array().unwrap().len(), 2);
        assert!(body["data"]["read_marker"].is_null());
        assert_eq!(body["data"]["pins"], json!([]));
        let gaps: Vec<(&str, &str)> = body["data"]["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|gap| (gap["section"].as_str().unwrap(), gap["code"].as_str().unwrap()))
            .collect();
        assert!(gaps.contains(&("read_marker", "read_marker_unavailable")), "{gaps:?}");
        assert!(gaps.contains(&("pins", "pins_unavailable")), "{gaps:?}");
    }

    #[tokio::test]
    async fn posts_outside_the_space_or_a_malformed_window_give_502() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (field, items) in [
            ("posts", json!([post("t9", "space-other", "2026-11-01T10:00:00Z")])),
            ("pinned", json!([post("t9", "space-other", "2026-11-01T10:00:00Z")])),
            ("refreshed", json!([post("t9", "space-other", "2026-11-01T10:00:00Z")])),
            ("posts", json!("not-an-array")),
        ] {
            let mut payload = window_payload();
            payload[field] = items;
            let fx = fixture(ResponseTemplate::new(200).set_body_json(payload), true).await;
            let (status, body) = timeline(&fx, TimelineQuery::default()).await;
            assert_eq!(status, 502, "{field}");
            assert_eq!(body["error"]["code"], "invalid_model_gateway_response");
        }
    }

    // ----- R2: the tail window of the transcript route ----------------------------------------

    fn window_query(tail: Option<&str>, before: Option<&str>, first: Option<&str>) -> TranscriptWindowQuery {
        TranscriptWindowQuery {
            tail: tail.map(str::to_owned),
            before: before.map(str::to_owned),
            include_first: first.map(str::to_owned),
        }
    }

    #[test]
    fn transcript_window_params_validated() {
        for bad in ["0", "51", "abc", "-1", "1.5"] {
            let (status, code) =
                code_of(parse_transcript_window(true, &window_query(Some(bad), None, None)));
            assert_eq!((status, code.as_str()), (400, "invalid_window"), "tail {bad}");
        }
        for (before, first) in [(Some("a b"), None), (Some("../x"), None), (None, Some("junk")), (None, Some("2"))] {
            let (status, code) =
                code_of(parse_transcript_window(true, &window_query(Some("12"), before, first)));
            assert_eq!((status, code.as_str()), (400, "invalid_window"), "{before:?}/{first:?}");
        }
        // before or include_first without tail is refused, not ignored.
        let (status, code) = code_of(parse_transcript_window(true, &window_query(None, Some("m-1"), None)));
        assert_eq!((status, code.as_str()), (400, "invalid_window"));
        let (status, _) = code_of(parse_transcript_window(true, &window_query(None, None, Some("1"))));
        assert_eq!(status, 400);
        // Bounds are accepted.
        for ok in ["1", "50"] {
            assert!(parse_transcript_window(true, &window_query(Some(ok), None, None)).ok().unwrap().is_some());
        }
        // No parameters at all: the whole transcript, no window.
        assert_eq!(parse_transcript_window(true, &window_query(None, None, None)).ok().unwrap(), None);
        assert_eq!(parse_transcript_window(true, &window_query(Some(" "), Some(""), None)).ok().unwrap(), None);
        // Flag off: everything is ignored, even junk.
        assert_eq!(
            parse_transcript_window(false, &window_query(Some("junk"), Some("a b"), Some("x"))).ok().unwrap(),
            None
        );
    }

    #[test]
    fn transcript_window_query_suffix_carries_limit_before_and_include_first() {
        let suffix = |tail, before, first| {
            parse_transcript_window(true, &window_query(tail, before, first))
                .ok()
                .unwrap()
                .unwrap()
                .query_suffix()
        };
        assert_eq!(suffix(Some("12"), None, None), "&limit=12");
        assert_eq!(suffix(Some("12"), Some("msg-1"), None), "&limit=12&before=msg-1");
        assert_eq!(suffix(Some("12"), None, Some("1")), "&limit=12&include_first=1");
        assert_eq!(suffix(Some("12"), None, Some("true")), "&limit=12&include_first=1");
        assert_eq!(suffix(Some("12"), None, Some("0")), "&limit=12");
        // A before cursor wins: the first turn is only wanted on the opening tail.
        assert_eq!(suffix(Some("12"), Some("msg-1"), Some("1")), "&limit=12&before=msg-1");
        assert_eq!(TAIL_WINDOW_TURNS, 12);
    }

    async fn model_url(fx: &Fx) -> String {
        fx.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .rev()
            .find(|request| request.url.path().ends_with("/messages"))
            .unwrap()
            .url
            .to_string()
    }

    async fn transcript(fx: &Fx, q: TranscriptWindowQuery) -> (u16, Value) {
        let (status, Json(body)) = super::super::space_thread_transcript(
            State(fx.state.clone()),
            Extension(member()),
            HeaderMap::new(),
            Path((SPACE.to_owned(), "thread-1".to_owned())),
            Query(q),
        )
        .await;
        (status.as_u16(), body)
    }

    #[tokio::test]
    async fn the_transcript_route_forwards_the_tail_window_and_reports_it_only_when_asked() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        mount(
            &fx.server,
            "GET",
            "/v1/threads/thread-1/messages",
            ResponseTemplate::new(200).set_body_json(json!({
                "messages": [{ "id": "m-1", "role": "user" }],
                "has_more": true,
                "omitted_before": 7,
            })),
        )
        .await;
        let (status, body) = transcript(&fx, window_query(Some("12"), None, Some("1"))).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["transcript"]["window"], json!({ "omitted_before": 7, "has_more_before": true }));
        let url = model_url(&fx).await;
        assert!(url.contains("limit=12") && url.contains("include_first=1"), "{url}");

        let (_, _) = transcript(&fx, window_query(Some("12"), Some("m-1"), None)).await;
        let url = model_url(&fx).await;
        assert!(url.contains("limit=12") && url.contains("before=m-1"), "{url}");

        // No window parameters: the whole transcript, no `window` object and no limit.
        let (status, body) = transcript(&fx, window_query(None, None, None)).await;
        assert_eq!(status, 200);
        assert!(body["data"]["transcript"].get("window").is_none());
        let url = model_url(&fx).await;
        assert!(!url.contains("limit=") && !url.contains("include_first"), "{url}");

        // Flag off: parameters are ignored (junk included) and the transcript is whole.
        let mut off = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        off.state.space_features.timeline_api = false;
        mount(
            &off.server,
            "GET",
            "/v1/threads/thread-1/messages",
            ResponseTemplate::new(200).set_body_json(json!({ "messages": [] })),
        )
        .await;
        let (status, body) = transcript(&off, window_query(Some("junk"), None, None)).await;
        assert_eq!(status, 200);
        assert!(body["data"]["transcript"].get("window").is_none());
        let url = model_url(&off).await;
        assert!(!url.contains("limit="), "{url}");

        // A bad window is a 400 before any upstream call.
        let fresh = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let (status, body) = transcript(&fresh, window_query(Some("51"), None, None)).await;
        assert_eq!((status, body["error"]["code"].clone()), (400, json!("invalid_window")));
        assert!(fresh.server.received_requests().await.unwrap().is_empty());
    }

    // ----- R5: the Space context features -----------------------------------------------------

    #[tokio::test]
    async fn space_context_features_include_timeline_flags() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let context = |fx: &Fx| {
            let state = fx.state.clone();
            async move {
                let (status, Json(body)) = super::super::space_context(
                    State(state),
                    Extension(member()),
                    Path(SPACE.to_owned()),
                )
                .await;
                (status.as_u16(), body)
            }
        };
        // Everything off: the new blocks exist and report false.
        fx.state.space_features.timeline_api = false;
        let (status, body) = context(&fx).await;
        assert_eq!(status, 200);
        let features = &body["data"]["features"];
        for key in ["timeline_windows", "system_entries", "timeline_anchor", "composer_drafts"] {
            assert_eq!(features[key], false, "{key}");
        }
        assert_eq!(body["data"]["limits"]["tail_window_turns"], 12);
        assert_eq!(features["link_previews"], false);

        // timeline_api alone turns windows on; system_entries needs both flags.
        fx.state.space_features.timeline_api = true;
        let (_, body) = context(&fx).await;
        assert_eq!(body["data"]["features"]["timeline_windows"], true);
        assert_eq!(body["data"]["features"]["system_entries"], false);
        fx.state.space_features.system_entries_api = true;
        let (_, body) = context(&fx).await;
        assert_eq!(body["data"]["features"]["system_entries"], true);
        fx.state.space_features.timeline_api = false;
        let (_, body) = context(&fx).await;
        assert_eq!(body["data"]["features"]["system_entries"], false);
    }

    /// WS10-S4 / WS13-S3 / WS14-S7: the context block mirrors the approval, search, link, palette,
    /// fork and room-file flags, each reported independently.
    #[tokio::test]
    async fn space_context_features_mirror_search_links_palette_fork_and_room_files() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        let context = |fx: &Fx| {
            let state = fx.state.clone();
            async move {
                let (_, Json(body)) = super::super::space_context(
                    State(state),
                    Extension(member()),
                    Path(SPACE.to_owned()),
                )
                .await;
                body
            }
        };
        let body = context(&fx).await;
        let features = &body["data"]["features"];
        for key in ["message_search", "entry_links", "command_palette", "thread_fork"] {
            assert_eq!(features[key], false, "{key} defaults off");
        }
        assert_eq!(
            features["room_files"],
            json!({ "turn_attachments": false, "durable": false, "files_tab": false })
        );
        assert_eq!(body["data"]["limits"]["room_files"]["max_files"], 10);

        // message_search needs the API and the UI flag.
        fx.state.space_features.message_search_api = true;
        assert_eq!(context(&fx).await["data"]["features"]["message_search"], false);
        fx.state.space_features.message_search_ui = true;
        assert_eq!(context(&fx).await["data"]["features"]["message_search"], true);
        fx.state.space_features.entry_links_ui = true;
        fx.state.space_features.command_palette_ui = true;
        fx.state.space_features.thread_fork_api = true;
        fx.state.space_features.room_files_api = true;
        fx.state.space_features.room_files_ui = true;
        fx.state.space_features.turn_attachments_ui = true;
        let body = context(&fx).await;
        let features = &body["data"]["features"];
        for key in ["entry_links", "command_palette", "thread_fork"] {
            assert_eq!(features[key], true, "{key}");
        }
        assert_eq!(
            features["room_files"],
            json!({ "turn_attachments": true, "durable": true, "files_tab": true })
        );
    }

    // ----- WS15-S12: room notices merged with Control's membership rows -----------------------

    fn control_row(id: &str, at: &str) -> Value {
        json!({ "id": id, "kind": "membership", "action": "member_added", "created_at": at })
    }

    fn notice_row(id: &str, at: &str) -> Value {
        json!({ "id": id, "kind": "notice", "action": "agent_bound", "created_at": at })
    }

    fn ids(rows: &[Value]) -> Vec<&str> {
        rows.iter().map(|row| row["id"].as_str().unwrap()).collect()
    }

    #[test]
    fn merge_system_rows_orders_by_created_at_then_id_newest_first() {
        let control = vec![
            control_row("c-old", "2026-11-01T10:00:00.000Z"),
            control_row("c-tie", "2026-11-01T10:00:05.000Z"),
        ];
        let notices = vec![
            notice_row("n-new", "2026-11-01T10:00:09.000Z"),
            notice_row("n-tie", "2026-11-01T10:00:05.000Z"),
        ];
        let (rows, truncated) = super::merge_system_rows(Some((control, false)), Some((notices, false)));
        assert_eq!(ids(&rows), ["n-new", "n-tie", "c-tie", "c-old"]);
        assert!(!truncated);
    }

    #[test]
    fn merge_system_rows_caps_at_a_hundred_and_says_it_cut() {
        let make = |prefix: &str, base: u64, count: u64| -> Vec<Value> {
            (0..count)
                .map(|i| {
                    let second = base + i;
                    control_row(
                        &format!("{prefix}{i:03}"),
                        &format!("2026-11-01T{:02}:{:02}:{:02}.000Z", second / 3600, (second / 60) % 60, second % 60),
                    )
                })
                .collect()
        };
        let (rows, truncated) = super::merge_system_rows(
            Some((make("c", 0, 60), false)),
            Some((make("n", 100, 60), false)),
        );
        assert_eq!(rows.len(), 100);
        assert!(truncated, "the merge itself dropped rows");
        // The newest survive: all 60 notices (later times) and the 40 newest Control rows.
        assert_eq!(rows[0]["id"], "n059");
        assert_eq!(rows[99]["id"], "c020");
    }

    #[test]
    fn merge_system_rows_exactly_a_hundred_is_not_truncated() {
        let rows: Vec<Value> = (0..100)
            .map(|i| control_row(&format!("c{i:03}"), &format!("2026-11-01T10:{:02}:{:02}.000Z", i / 60, i % 60)))
            .collect();
        let (merged, truncated) = super::merge_system_rows(Some((rows, false)), None);
        assert_eq!(merged.len(), 100);
        assert!(!truncated);
    }

    #[test]
    fn merge_system_rows_truncated_when_either_source_was_cut() {
        let one = || vec![control_row("c", "2026-11-01T10:00:00.000Z")];
        let (_, from_control) = super::merge_system_rows(Some((one(), true)), Some((vec![], false)));
        assert!(from_control);
        let (_, from_notices) = super::merge_system_rows(Some((one(), false)), Some((vec![], true)));
        assert!(from_notices);
    }

    #[test]
    fn merge_system_rows_keeps_the_other_source_when_one_failed() {
        let control = vec![control_row("c", "2026-11-01T10:00:00.000Z")];
        let notices = vec![notice_row("n", "2026-11-01T10:00:00.000Z")];
        let (only_control, _) = super::merge_system_rows(Some((control, false)), None);
        assert_eq!(ids(&only_control), ["c"]);
        let (only_notices, _) = super::merge_system_rows(None, Some((notices, true)));
        assert_eq!(ids(&only_notices), ["n"]);
        let (none, truncated) = super::merge_system_rows(None, None);
        assert!(none.is_empty() && !truncated);
    }

    async fn notices_fixture(value: ResponseTemplate) -> Fx {
        let fx = fixture(ResponseTemplate::new(200).set_body_json(window_payload()), true).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(body_string_contains("roomNoticesForGateway"))
            .respond_with(value)
            .mount(&fx.server)
            .await;
        fx
    }

    #[tokio::test]
    async fn room_notices_keeps_known_kinds_and_drops_rows_without_id_or_time() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = notices_fixture(ResponseTemplate::new(200).set_body_json(json!({ "value": {
            "truncated": true,
            "notices": [
                { "noticeId": "rn_1", "kind": "agent_paused", "actorExternalAuthId": "alice", "subjectId": "agent-1", "createdAt": 1_762_000_000_000_i64 },
                { "noticeId": "rn_2", "kind": "topic_changed", "actorExternalAuthId": null, "subjectId": null, "createdAt": 1_762_000_001_000_i64 },
                { "noticeId": "rn_3", "kind": "party_started", "createdAt": 1_762_000_002_000_i64 },
                { "noticeId": "", "kind": "agent_bound", "createdAt": 1_762_000_003_000_i64 },
                { "kind": "agent_bound", "createdAt": 1_762_000_004_000_i64 },
                { "noticeId": "rn_4", "kind": "agent_bound" },
                { "noticeId": "rn_5", "kind": "agent_bound", "createdAt": "soon" },
            ]
        }}))).await;
        let (rows, truncated) = super::room_notices(&fx.state, &member(), "org-1", SPACE, (Some(10), Some(20)))
            .await
            .expect("Application answered");
        assert!(truncated, "Application's own cut is passed on");
        assert_eq!(ids(&rows), ["rn_1", "rn_2"]);
        assert_eq!(rows[0]["kind"], "notice");
        assert_eq!(rows[0]["action"], "agent_paused");
        assert_eq!(rows[0]["actor_id"], "alice");
        assert_eq!(rows[0]["subject_id"], "agent-1");
        assert_eq!(rows[1]["actor_id"], "", "absent ids are empty words, not names");
        assert!(rows[0]["created_at"].as_str().unwrap().ends_with('Z'));

        // The query carries the caller, the organization, the Space, the bound and the interval.
        let call = fx
            .server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| String::from_utf8_lossy(&request.body).contains("roomNoticesForGateway"))
            .expect("a roomNoticesForGateway call");
        let body: Value = serde_json::from_slice(&call.body).unwrap();
        assert_eq!(body["path"], "spaceRoomNotices:roomNoticesForGateway");
        assert_eq!(body["args"]["externalAuthId"], "timeline-user");
        assert_eq!(body["args"]["externalOrgId"], "org-1");
        assert_eq!(body["args"]["spaceRef"], SPACE);
        assert_eq!(body["args"]["limit"], 100);
        assert_eq!(body["args"]["after"], 10);
        assert_eq!(body["args"]["before"], 20);
    }

    #[tokio::test]
    async fn room_notices_is_none_when_application_fails_or_answers_nonsense() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for template in [
            ResponseTemplate::new(500),
            ResponseTemplate::new(200).set_body_json(json!({ "value": { "nope": [] } })),
            ResponseTemplate::new(200).set_body_json(json!({})),
        ] {
            let fx = notices_fixture(template).await;
            assert!(super::room_notices(&fx.state, &member(), "org-1", SPACE, (None, None)).await.is_none());
        }
    }

    async fn mount_control_events(fx: &Fx, template: ResponseTemplate) {
        mount(
            &fx.server,
            "GET",
            &format!("/api/v1/internal/spaces/{SPACE}/membership-events"),
            template,
        )
        .await;
    }

    fn control_events() -> Value {
        json!({ "data": { "events": [
            { "event_id": "ev-1", "action": "member_added", "space_seq": 1, "actor_id": "owner", "subject_id": "bob", "created_at": "2026-11-01T10:01:00.000Z" }
        ], "truncated": false }})
    }

    fn notice_answer() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({ "value": {
            "truncated": false,
            "notices": [{ "noticeId": "rn_1", "kind": "agent_bound", "actorExternalAuthId": "owner", "subjectId": "agent-1", "createdAt": 1793530800000_i64 }]
        }}))
    }

    fn gap_codes(body: &Value) -> Vec<String> {
        body["data"]["unavailable"]
            .as_array()
            .map(|gaps| gaps.iter().map(|gap| gap["code"].as_str().unwrap().to_owned()).collect())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn timeline_merges_control_rows_and_application_notices() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(notice_answer()).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(200).set_body_json(control_events())).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        let rows = body["data"]["system_entries"].as_array().unwrap();
        assert_eq!(ids(rows), ["rn_1", "ev-1"], "newest first across both sources");
        assert_eq!(rows[0]["kind"], "notice");
        assert_eq!(rows[1]["kind"], "membership");
        assert_eq!(body["data"]["system_truncated"], false);
        assert!(gap_codes(&body).is_empty(), "{:?}", gap_codes(&body));
    }

    #[tokio::test]
    async fn application_failing_is_a_room_notices_gap_that_keeps_control_rows() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(ResponseTemplate::new(503)).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(200).set_body_json(control_events())).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200, "the window is still served");
        assert_eq!(ids(body["data"]["system_entries"].as_array().unwrap()), ["ev-1"]);
        assert_eq!(gap_codes(&body), ["room_notices_unavailable"]);
        assert_eq!(body["data"]["threads"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn control_failing_keeps_the_notices_and_names_its_own_gap() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(notice_answer()).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(500)).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        assert_eq!(ids(body["data"]["system_entries"].as_array().unwrap()), ["rn_1"]);
        assert_eq!(gap_codes(&body), ["system_entries_unavailable"]);
    }

    #[tokio::test]
    async fn both_sources_failing_leaves_no_system_rows_and_two_gaps() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(ResponseTemplate::new(503)).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(500)).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["system_entries"], json!([]));
        assert!(body["data"].get("system_truncated").is_none());
        let codes = gap_codes(&body);
        assert!(codes.contains(&"system_entries_unavailable".to_owned()), "{codes:?}");
        assert!(codes.contains(&"room_notices_unavailable".to_owned()), "{codes:?}");
    }

    #[tokio::test]
    async fn with_the_system_entries_flag_off_neither_source_is_asked() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = notices_fixture(notice_answer()).await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["system_entries"], json!([]));
        assert_eq!(requests_to(&fx.server, "membership-events").await, 0);
        let notice_calls = fx
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| String::from_utf8_lossy(&request.body).contains("roomNoticesForGateway"))
            .count();
        assert_eq!(notice_calls, 0);
    }

    // ----- WS15-S11: the system rows are cut to the page interval ------------------------------

    fn window(before: (u64, Option<i64>), after: (u64, Option<i64>)) -> super::TimelineWindow {
        super::TimelineWindow {
            limit: 20,
            before_seq: before.0,
            after_seq: after.0,
            before_ms: before.1,
            after_ms: after.1,
            refresh: Vec::new(),
        }
    }

    #[test]
    fn system_entries_are_cut_to_the_page_interval() {
        // first page: open at the top, rows newer than the newest post still show
        let first = window((0, None), (0, None));
        assert_eq!(super::system_interval(&first, true, Some(100), Some(900)), (Some(100), None));
        assert_eq!(super::system_interval(&first, false, Some(100), Some(900)), (None, None), "reached the start of the room");
        // older page: from its own oldest post up to the time the before-cursor carries
        let older = window((5, Some(500)), (0, None));
        assert_eq!(super::system_interval(&older, true, Some(300), Some(480)), (Some(300), Some(500)));
        assert_eq!(super::system_interval(&older, false, Some(300), Some(480)), (None, Some(500)), "has_more=false leaves lo open");
        // an old cursor without a time leaves the upper bound open
        let legacy = window((5, None), (0, None));
        assert_eq!(super::system_interval(&legacy, true, Some(300), None), (Some(300), None));
        // refresh page: from the cursor's time upward; a cut window stops at its newest post
        let after = window((0, None), (9, Some(900)));
        assert_eq!(super::system_interval(&after, false, Some(910), Some(990)), (Some(900), None));
        assert_eq!(super::system_interval(&after, true, Some(910), Some(990)), (Some(900), Some(991)));
    }

    #[test]
    fn a_legacy_cursor_without_a_time_still_decodes_and_a_bad_time_does_not() {
        assert_eq!(decode_cursor(&cursor("v1|7")), Some((7, None)));
        assert_eq!(decode_cursor(&cursor("v1|7|1793527200000")), Some((7, Some(1_793_527_200_000))));
        assert_eq!(decode_cursor(&cursor("v1|7|soon")), None);
        assert_eq!(decode_cursor(&cursor("v1|0|5")), None);
    }

    async fn membership_event_queries(fx: &Fx) -> Vec<Vec<(String, String)>> {
        fx.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path().ends_with("/membership-events"))
            .map(|request| {
                request
                    .url
                    .query_pairs()
                    .map(|(key, value)| (key.into_owned(), value.into_owned()))
                    .collect()
            })
            .collect()
    }

    #[tokio::test]
    async fn the_control_read_carries_the_interval_of_an_older_page() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(ResponseTemplate::new(503)).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(200).set_body_json(control_events())).await;
        let before_ms = 1_793_530_800_000_i64; // 2026-11-01T11:00:00Z, after both posts
        let query = TimelineQuery {
            before: Some(super::encode_cursor(5, Some(before_ms)).as_str().unwrap().to_owned()),
            ..TimelineQuery::default()
        };
        let (status, _) = timeline(&fx, query).await;
        assert_eq!(status, 200);
        let queries = membership_event_queries(&fx).await;
        assert_eq!(queries.len(), 1);
        let get = |name: &str| queries[0].iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
        assert_eq!(get("limit").as_deref(), Some("100"));
        // hi is the time the before-cursor carries; lo is this page's own oldest post (has_more)
        assert_eq!(get("before"), super::rfc3339_from_ms(before_ms));
        assert_eq!(get("after").as_deref(), Some("2026-11-01T10:00:00.000Z"));
    }

    #[tokio::test]
    async fn the_first_page_is_open_at_the_top_and_a_refresh_starts_at_the_cursor_time() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(ResponseTemplate::new(503)).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(&fx, ResponseTemplate::new(200).set_body_json(control_events())).await;
        let (status, _) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        let first = membership_event_queries(&fx).await;
        assert!(first[0].iter().all(|(key, _)| key != "before"), "no upper bound on the first page: {first:?}");
        assert!(first[0].iter().any(|(key, _)| key == "after"), "has_more=true bounds it by the page's oldest post");

        let after_ms = 1_793_527_800_000_i64;
        let query = TimelineQuery {
            after: Some(super::encode_cursor(9, Some(after_ms)).as_str().unwrap().to_owned()),
            ..TimelineQuery::default()
        };
        let (status, _) = timeline(&fx, query).await;
        assert_eq!(status, 200);
        let all = membership_event_queries(&fx).await;
        let refresh = all.last().unwrap();
        let after = refresh.iter().find(|(key, _)| key == "after").map(|(_, value)| value.clone());
        assert_eq!(after, super::rfc3339_from_ms(after_ms), "a refresh covers from the cursor's post time upward");
    }

    #[tokio::test]
    async fn an_unknown_action_and_a_row_without_an_event_id_are_dropped() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = notices_fixture(ResponseTemplate::new(503)).await;
        fx.state.space_features.system_entries_api = true;
        mount_control_events(
            &fx,
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "events": [
                { "event_id": "ev-1", "action": "member_added", "space_seq": 1, "created_at": "2026-11-01T10:01:00.000Z" },
                { "event_id": "ev-2", "action": "ownership_transferred", "created_at": "2026-11-01T10:02:00.000Z" },
                { "event_id": "", "action": "member_added", "created_at": "2026-11-01T10:03:00.000Z" },
                { "action": "member_removed", "created_at": "2026-11-01T10:04:00.000Z" },
                { "event_id": "ev-5", "action": "settings_changed", "created_at": "2026-11-01T10:05:00.000Z" }
            ], "truncated": true }})),
        )
        .await;
        let (status, body) = timeline(&fx, TimelineQuery::default()).await;
        assert_eq!(status, 200);
        let rows = body["data"]["system_entries"].as_array().unwrap();
        assert_eq!(ids(rows), ["ev-5", "ev-1"], "newest first, only the closed action set");
        assert_eq!(body["data"]["system_truncated"], true, "Control's cut is relayed");
        assert_eq!(rows[1]["kind"], "membership");
    }
}
