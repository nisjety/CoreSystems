//! `POST /v1/spaces/:space_id/messages/search` and `.../messages/resolve` (build plan WS13-S2):
//! permission-checked Space message search and message address resolution.
//!
//! A thin relay of Session Core's `SpaceHistoryService`. Authority is Session Core's: these routes
//! forward the caller's verified bearer plus the Control read decision pair, which travels in the
//! POST body (never a URL, so neither the decision token nor the query text reaches an access
//! log). `org_id` and `user_id` come from the verified claims, never the body. Both are reads: a
//! ZDR caller may call them, so there is no ZDR refusal, no cache, no idempotency record and no
//! event. Query text, snippets and ids are never logged, and every response is `no-store`. User
//! principals only.
//!
//! Errors, as `{error:{code,message,details}}`: flag off (or Session Core's switch off) 404
//! `space_search_disabled`, `UNAUTHENTICATED` 401 `unauthorized`, a malformed or unknown-field body
//! 400 `invalid_request`, `INVALID_ARGUMENT` 400 `invalid_query` / `invalid_filter` /
//! `invalid_cursor`, `PERMISSION_DENIED` 403 `space_read_not_authorized`, `NOT_FOUND` 404
//! `entry_not_found`, `RESOURCE_EXHAUSTED` 429 `search_busy`, `DEADLINE_EXCEEDED` 503
//! `search_timeout`, `UNAVAILABLE` 503 `search_unavailable`, anything else 503
//! `session_unavailable`. Messages are fixed words, never the upstream text.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    ResolveSpaceMessageRequest, SearchSpaceMessagesRequest, SnippetSegment, SpaceMessageHit,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{authenticated_session_request, timestamp_to_rfc3339};
use crate::state::AppState;

const MAX_SPACE_ID_BYTES: usize = 128;
const MAX_ID_BYTES: usize = 128;
/// Session Core limits the query to 200 characters; this only bounds what is forwarded.
const MAX_QUERY_BYTES: usize = 1024;
const MAX_CURSOR_BYTES: usize = 2048;
const MAX_DECISION_BYTES: usize = 8192;
/// Session Core's own trailer for a closed error word (`run_phase::ERROR_CODE_TRAILER`).
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";

/// The two Space history routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/spaces/:space_id/messages/search", post(search_messages))
        .route("/v1/spaces/:space_id/messages/resolve", post(resolve_message))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchBody {
    query: String,
    #[serde(default)]
    prefix: bool,
    /// `room` (default), `thread` or `person`.
    scope: Option<String>,
    thread_id: Option<String>,
    author_subject_id: Option<String>,
    role: Option<String>,
    since: Option<String>,
    until: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveBody {
    message_id: String,
    space_read_decision_ref: Option<String>,
    space_read_decision_token: Option<String>,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let mut response = (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
        .into_response();
    no_store(&mut response);
    response
}

fn no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

fn invalid_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn invalid_filter(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_filter", message)
}

fn disabled() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "space_search_disabled",
        "Space message search is not enabled",
    )
}

/// The closed error word Session Core attached to a refusal, when it is one this route knows.
fn upstream_code(status: &tonic::Status) -> Option<&'static str> {
    let raw = status.metadata().get(ERROR_CODE_TRAILER)?.to_str().ok()?;
    [
        "invalid_cursor",
        "invalid_filter",
        "invalid_query",
        "space_search_disabled",
        "search_busy",
        "search_timeout",
        "search_unavailable",
        "entry_not_found",
    ]
    .into_iter()
    .find(|known| *known == raw)
}

/// The HTTP form of an upstream refusal. Messages are fixed words, never the upstream text.
fn status_response(status: &tonic::Status) -> Response {
    let code = upstream_code(status);
    if code == Some("space_search_disabled") || status.code() == tonic::Code::Unimplemented {
        return disabled();
    }
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::InvalidArgument => match code {
            Some("invalid_cursor") => error_response(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the search cursor is invalid",
            ),
            Some("invalid_filter") => invalid_filter("a search filter is invalid"),
            _ => error_response(
                StatusCode::BAD_REQUEST,
                "invalid_query",
                "the search request is invalid",
            ),
        },
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "reading this Space is not permitted",
        ),
        tonic::Code::NotFound => {
            error_response(StatusCode::NOT_FOUND, "entry_not_found", "entry not found")
        }
        tonic::Code::ResourceExhausted => error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "search_busy",
            "search is busy, try again shortly",
        ),
        tonic::Code::DeadlineExceeded => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_timeout",
            "the search timed out",
        ),
        tonic::Code::Unavailable => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_unavailable",
            "search is unavailable",
        ),
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "the session service is unavailable",
        ),
    }
}

/// A trimmed, bounded Space id from the path.
fn bounded_space_id(raw: &str) -> Result<String, Response> {
    let space_id = raw.trim().to_owned();
    if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
        return Err(invalid_request("space_id is invalid"));
    }
    Ok(space_id)
}

/// The decision pair, bounded. Session Core verifies it; this only bounds what is forwarded.
fn decision_pair(
    reference: Option<String>,
    token: Option<String>,
) -> Result<(String, String), Response> {
    let reference = reference.unwrap_or_default();
    let token = token.unwrap_or_default();
    if reference.len() > MAX_DECISION_BYTES || token.len() > MAX_DECISION_BYTES {
        return Err(invalid_request("the read decision is too large"));
    }
    Ok((reference, token))
}

fn parse_time(raw: Option<&str>) -> Result<Option<prost_types::Timestamp>, Response> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };
    let parsed = chrono::DateTime::parse_from_rfc3339(raw)
        .map_err(|_| invalid_filter("since and until must be RFC 3339 timestamps"))?;
    Ok(Some(prost_types::Timestamp {
        seconds: parsed.timestamp(),
        nanos: i32::try_from(parsed.timestamp_subsec_nanos()).unwrap_or(0),
    }))
}

fn bounded_filter_id(raw: Option<String>, what: &'static str) -> Result<String, Response> {
    let value = raw.unwrap_or_default().trim().to_owned();
    if value.len() > MAX_ID_BYTES {
        return Err(invalid_filter(what));
    }
    Ok(value)
}

fn segment_json(segment: &SnippetSegment) -> Value {
    json!({ "text": segment.text, "matched": segment.matched })
}

fn hit_json(hit: &SpaceMessageHit) -> Value {
    let mut item = json!({
        "message_id": hit.message_id,
        "thread_id": hit.thread_id,
        "space_id": hit.space_id,
        "role": hit.role,
        "created_at": timestamp_to_rfc3339(hit.created_at.clone()),
        "snippet": hit.snippet.iter().map(segment_json).collect::<Vec<_>>(),
        "thread_title": hit.thread_title,
    });
    if !hit.author_subject_id.is_empty() {
        item["author_subject_id"] = json!(hit.author_subject_id);
    }
    if !hit.agent_name.is_empty() {
        item["agent_name"] = json!(hit.agent_name);
    }
    item
}

async fn search_messages(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    body: Result<Json<SearchBody>, JsonRejection>,
) -> Response {
    // 1. Rollout flag. A rollout control only.
    if !state.space_message_search_api_enabled {
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
    let space_id = match bounded_space_id(&space_id) {
        Ok(space_id) => space_id,
        Err(response) => return response,
    };
    // 3. A strict body (unknown fields are a 400, never a silent drop).
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => return invalid_request("the search body is invalid"),
    };
    if body.query.len() > MAX_QUERY_BYTES {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "the search query is too long",
        );
    }
    if body.cursor.as_deref().map_or(0, str::len) > MAX_CURSOR_BYTES {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "the search cursor is invalid",
        );
    }
    let (decision_ref, decision_token) =
        match decision_pair(body.space_read_decision_ref, body.space_read_decision_token) {
            Ok(pair) => pair,
            Err(response) => return response,
        };
    // 4. Filters. The scope picks which of `thread_id` / `author_subject_id` applies; the others
    //    are not forwarded, so a stale client value cannot widen or skew a query.
    let mut thread_id = bounded_filter_id(body.thread_id, "thread_id is invalid");
    let mut author_subject_id =
        bounded_filter_id(body.author_subject_id, "author_subject_id is invalid");
    match body.scope.as_deref().map(str::trim).unwrap_or("room") {
        "" | "room" => {
            thread_id = Ok(String::new());
            author_subject_id = Ok(String::new());
        }
        "thread" => {
            author_subject_id = Ok(String::new());
            if thread_id.as_ref().map_or(true, String::is_empty) {
                return invalid_filter("scope thread needs a thread_id");
            }
        }
        "person" => {
            thread_id = Ok(String::new());
            if author_subject_id.as_ref().map_or(true, String::is_empty) {
                return invalid_filter("scope person needs an author_subject_id");
            }
        }
        _ => return invalid_filter("scope must be room, thread or person"),
    }
    let thread_id = match thread_id {
        Ok(value) => value,
        Err(response) => return response,
    };
    let author_subject_id = match author_subject_id {
        Ok(value) => value,
        Err(response) => return response,
    };
    let role = body.role.as_deref().map(str::trim).unwrap_or_default();
    if !matches!(role, "" | "user" | "assistant") {
        return invalid_filter("role must be user or assistant");
    }
    let sort = body.sort.as_deref().map(str::trim).unwrap_or_default();
    if !matches!(sort, "" | "recent" | "relevance") {
        return invalid_filter("sort must be recent or relevance");
    }
    let since = match parse_time(body.since.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let until = match parse_time(body.until.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = match authenticated_session_request(
        SearchSpaceMessagesRequest {
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
            space_id,
            space_read_decision_ref: decision_ref,
            space_read_decision_token: decision_token,
            query: body.query,
            prefix: body.prefix,
            thread_id,
            author_subject_id,
            role: role.to_owned(),
            since,
            until,
            sort: sort.to_owned(),
            limit: body.limit.unwrap_or(0),
            cursor: body.cursor.unwrap_or_default(),
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    let response = match state.space_history_client.clone().search_messages(request).await {
        Ok(response) => response.into_inner(),
        Err(status) => return status_response(&status),
    };
    let next_cursor = (!response.next_cursor.is_empty()).then_some(response.next_cursor);
    let mut out = Json(json!({
        "hits": response.hits.iter().map(hit_json).collect::<Vec<_>>(),
        "next_cursor": next_cursor,
        "truncated": response.truncated,
        "coverage": response.coverage,
        "excluded": response.excluded,
    }))
    .into_response();
    no_store(&mut out);
    out
}

async fn resolve_message(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    body: Result<Json<ResolveBody>, JsonRejection>,
) -> Response {
    if !state.space_message_search_api_enabled {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "a user principal is required",
        );
    }
    let space_id = match bounded_space_id(&space_id) {
        Ok(space_id) => space_id,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => return invalid_request("the resolve body is invalid"),
    };
    let message_id = body.message_id.trim().to_owned();
    // A malformed id is the same answer as a missing one: the route never says which.
    if message_id.is_empty() || message_id.len() > MAX_ID_BYTES {
        return error_response(StatusCode::NOT_FOUND, "entry_not_found", "entry not found");
    }
    let (decision_ref, decision_token) =
        match decision_pair(body.space_read_decision_ref, body.space_read_decision_token) {
            Ok(pair) => pair,
            Err(response) => return response,
        };
    let request = match authenticated_session_request(
        ResolveSpaceMessageRequest {
            org_id: claims.org_id.clone(),
            user_id: claims.user_id.clone(),
            space_id,
            space_read_decision_ref: decision_ref,
            space_read_decision_token: decision_token,
            message_id,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    let response = match state.space_history_client.clone().resolve_message(request).await {
        Ok(response) => response.into_inner(),
        Err(status) => return status_response(&status),
    };
    let mut out = Json(json!({
        "message_id": response.message_id,
        "thread_id": response.thread_id,
        "space_id": response.space_id,
    }))
    .into_response();
    no_store(&mut out);
    out
}
