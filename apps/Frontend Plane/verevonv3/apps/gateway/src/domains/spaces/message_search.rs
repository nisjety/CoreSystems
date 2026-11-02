//! Space message search, entry addresses and the feature report: the BFF half of WS13-S3.
//!
//! * `POST /api/v1/spaces/{space_ref}/search` is permission-checked message search. The query and
//!   the Control read decision travel in POST bodies (never a URL), the response is
//!   `Cache-Control: no-store`, and nothing derived from the query is cached, keyed, stored or
//!   logged (no use of the web-search cache; finding F-t2-003 is not reused). Counts and status are
//!   the only things that reach a log line.
//! * `GET /api/v1/spaces/{space_ref}/entries/{message_id}` resolves a message address to its
//!   thread, under the same read decision. A hidden, missing and foreign message answer alike.
//! * `GET /api/v1/spaces/features` reports the shell-level rollout flags where no Space is in
//!   scope. A rollout report only, never an authorization input.
//!
//! Authority is not here. The BFF mints a FRESH Control read decision per request and forwards it;
//! model-gateway relays it to Session Core, which verifies it and applies the audience predicates
//! in the same SQL statement. A Control decline (403/404) is "no shared read": a search is then
//! answered owner-only (`coverage = own_threads_only`) after the caller's membership is confirmed
//! live, while the entries route answers 403. An unreachable Control is an error (503
//! `space_decision_unavailable`), never "not entitled". After the answer, the BFF asserts every
//! hit belongs to the requested Space and refuses the whole response with 502 otherwise.
//!
//! ZDR: these are reads, so a Zero Data Retention caller may search persisted shared content; the
//! routes persist nothing.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_query` (details
//! `{min_chars,max_chars}`) / `invalid_filter` / `invalid_cursor` / `invalid_space`, 403
//! `active_membership_required` / `space_read_not_authorized`, 404 `space_not_found` /
//! `space_search_disabled` / `entry_not_found`, 429 `search_busy`, 502
//! `invalid_model_gateway_response`, 503 `space_lifecycle_unavailable` /
//! `space_decision_unavailable` / `delegated_auth_unavailable` / `search_timeout` /
//! `search_unavailable`.

use axum::{
    extract::{rejection::JsonRejection, Extension, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{
    denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref,
};
use crate::{
    config::AppState, envelope::error_with_details, middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

const QUERY_MIN_CHARS: usize = 2;
const QUERY_MAX_CHARS: usize = 200;
const MAX_ID_LEN: usize = 200;
const MAX_CURSOR_LEN: usize = 2048;
const MAX_TIME_LEN: usize = 64;

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        // A static segment: matchit ranks it above `{space_ref}`, so a room can never shadow it.
        .route("/api/v1/spaces/features", get(space_features))
        .route("/api/v1/spaces/{space_ref}/search", post(search_space_messages))
        .route(
            "/api/v1/spaces/{space_ref}/entries/{message_id}",
            get(resolve_space_entry),
        )
}

/// Strict body: an unknown field is a 400, never a silent drop.
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
}

fn reply(status: StatusCode, body: Value) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    reply(status, error_with_details(code, message, json!({})))
}

fn disabled() -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        "space_search_disabled",
        "Space message search is not enabled.",
    )
}

fn invalid_filter(message: &'static str) -> Response {
    refuse(StatusCode::BAD_REQUEST, "invalid_filter", message)
}

fn entry_not_found() -> Response {
    refuse(StatusCode::NOT_FOUND, "entry_not_found", "Entry not found.")
}

fn bad_gateway() -> Response {
    refuse(
        StatusCode::BAD_GATEWAY,
        "invalid_model_gateway_response",
        "The search service returned an unexpected response.",
    )
}

fn search_unavailable() -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "search_unavailable",
        "Search is unavailable.",
    )
}

/// An id-like value: bounded, trimmed, no control characters.
fn clean_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

/// A message id as the SPA address grammar allows it (`[A-Za-z0-9_-]{1,64}`).
fn valid_message_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The organization and Space lifecycle checks both routes share. `Ok(org_id, space_ref)`.
async fn authorize_scope(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Response> {
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to search a Space.",
        ));
    }
    let Some(space_ref) = clean_id(space_ref) else {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    };
    match space_lifecycle_by_ref(state, user, &org_id, &space_ref).await {
        Ok(Some(_)) => Ok((org_id, space_ref)),
        Ok(None) => Err(refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "Space is not available.",
        )),
        Err(()) => Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_lifecycle_unavailable",
            "Space lifecycle is unavailable.",
        )),
    }
}

/// A fresh Control read decision for this request. `Ok(None)` is a decline (no shared read);
/// the second value is the typed reason when Control gave one. An unreachable or erroring
/// Control is an error and is never read as "not entitled".
async fn mint_read_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<(Option<(String, String)>, Option<denial::SpaceDenial>), Response> {
    match shared_thread_read_decision_for_typed(state, user, org_id, space_ref, "").await {
        Ok(decision) => Ok(decision.split()),
        Err(_) => Err(refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_decision_unavailable",
            "The Space read decision is unavailable.",
        )),
    }
}

/// The caller's Session Core and model-gateway bearers.
async fn delegated_bearers(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<(String, Option<String>), Response> {
    let session_token = match crate::domains::chat::shared::required_session_token(
        state, user, headers,
    )
    .await
    {
        Ok(token) => token,
        Err(reason) => {
            let (status, Json(body)) =
                crate::domains::chat::shared::delegated_auth_unavailable(reason);
            return Err(reply(status, body));
        }
    };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    Ok((session_token, model_token))
}

/// The closed error word model-gateway attached, when it is one this BFF knows.
fn upstream_code(body: &Value) -> Option<&'static str> {
    let raw = body.get("error")?.get("code")?.as_str()?;
    [
        "invalid_query",
        "invalid_filter",
        "invalid_cursor",
        "space_read_not_authorized",
        "space_search_disabled",
        "entry_not_found",
        "search_busy",
        "search_timeout",
        "search_unavailable",
    ]
    .into_iter()
    .find(|known| *known == raw)
}

/// The BFF form of a model-gateway refusal. Messages are fixed words, never the upstream text.
/// `entries` selects how an unrecognized 404 reads: for a search it means the capability is
/// absent; for an entry it means the entry is not there.
fn map_model_error(status: StatusCode, body: &Value, entries: bool) -> Response {
    let code = upstream_code(body);
    match status {
        StatusCode::NOT_FOUND => {
            if code == Some("entry_not_found") || (entries && code.is_none()) {
                entry_not_found()
            } else {
                disabled()
            }
        }
        StatusCode::BAD_REQUEST => match code {
            Some("invalid_cursor") => refuse(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "The search cursor is invalid.",
            ),
            Some("invalid_filter") => invalid_filter("A search filter is invalid."),
            Some("invalid_query") => query_invalid(),
            // The BFF validated the request first, so anything else is a contract mismatch.
            _ => bad_gateway(),
        },
        StatusCode::UNAUTHORIZED => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "delegated_auth_unavailable",
            "Required session authentication is temporarily unavailable.",
        ),
        StatusCode::FORBIDDEN => refuse(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "Reading this Space is not authorized.",
        ),
        StatusCode::TOO_MANY_REQUESTS => refuse(
            StatusCode::TOO_MANY_REQUESTS,
            "search_busy",
            "Search is busy. Try again shortly.",
        ),
        StatusCode::GATEWAY_TIMEOUT => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_timeout",
            "The search timed out.",
        ),
        status if status.is_server_error() => {
            if code == Some("search_timeout") {
                refuse(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "search_timeout",
                    "The search timed out.",
                )
            } else {
                search_unavailable()
            }
        }
        _ => bad_gateway(),
    }
}

fn query_invalid() -> Response {
    reply(
        StatusCode::BAD_REQUEST,
        error_with_details(
            "invalid_query",
            "The search text must be between 2 and 200 characters.",
            json!({ "min_chars": QUERY_MIN_CHARS, "max_chars": QUERY_MAX_CHARS }),
        ),
    )
}

/// Validated, forwardable form of the search body (the decision pair is added later).
struct ValidSearch {
    forward: Map<String, Value>,
}

fn validate_search(body: SearchBody) -> Result<ValidSearch, Response> {
    let query = body.query.trim();
    let length = query.chars().count();
    if !(QUERY_MIN_CHARS..=QUERY_MAX_CHARS).contains(&length)
        || query.chars().any(char::is_control)
    {
        return Err(query_invalid());
    }
    if body.cursor.as_deref().is_some_and(|cursor| {
        cursor.len() > MAX_CURSOR_LEN || cursor.chars().any(char::is_control)
    }) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "The search cursor is invalid.",
        ));
    }
    let scope = body.scope.as_deref().map(str::trim).unwrap_or("room");
    let optional_id = |raw: &Option<String>| -> Result<Option<String>, Response> {
        match raw.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(None),
            Some(value) => clean_id(value)
                .map(Some)
                .ok_or_else(|| invalid_filter("A search filter is invalid.")),
        }
    };
    let thread_id = optional_id(&body.thread_id)?;
    let author_subject_id = optional_id(&body.author_subject_id)?;
    // The scope picks which filter applies; the other is not forwarded, so a stale client value
    // cannot widen or skew a query.
    let (thread_id, author_subject_id) = match scope {
        "" | "room" => (None, None),
        "thread" => match thread_id {
            Some(thread_id) => (Some(thread_id), None),
            None => return Err(invalid_filter("Scope thread needs a thread_id.")),
        },
        "person" => match author_subject_id {
            Some(author) => (None, Some(author)),
            None => return Err(invalid_filter("Scope person needs an author_subject_id.")),
        },
        _ => return Err(invalid_filter("Scope must be room, thread or person.")),
    };
    let role = body.role.as_deref().map(str::trim).filter(|v| !v.is_empty());
    if role.is_some_and(|role| !matches!(role, "user" | "assistant")) {
        return Err(invalid_filter("Role must be user or assistant."));
    }
    let sort = body.sort.as_deref().map(str::trim).filter(|v| !v.is_empty());
    if sort.is_some_and(|sort| !matches!(sort, "recent" | "relevance")) {
        return Err(invalid_filter("Sort must be recent or relevance."));
    }
    let time = |raw: &Option<String>| -> Result<Option<String>, Response> {
        match raw.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            None => Ok(None),
            Some(value) if value.len() <= MAX_TIME_LEN && !value.chars().any(char::is_control) => {
                Ok(Some(value.to_owned()))
            }
            Some(_) => Err(invalid_filter("Since and until must be RFC 3339 timestamps.")),
        }
    };
    let since = time(&body.since)?;
    let until = time(&body.until)?;

    let mut forward = Map::new();
    forward.insert("query".into(), json!(query));
    forward.insert("prefix".into(), json!(body.prefix));
    forward.insert("scope".into(), json!(if scope.is_empty() { "room" } else { scope }));
    let mut put = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            forward.insert(key.into(), Value::String(value));
        }
    };
    put("thread_id", thread_id);
    put("author_subject_id", author_subject_id);
    put("role", role.map(str::to_owned));
    put("sort", sort.map(str::to_owned));
    put("since", since);
    put("until", until);
    put("cursor", body.cursor.filter(|cursor| !cursor.is_empty()));
    if let Some(limit) = body.limit {
        forward.insert("limit".into(), json!(limit));
    }
    Ok(ValidSearch { forward })
}

/// Every hit must belong to the requested Space. The index proposes candidates and Session Core
/// decides, but a response that names another Space is a defect worth refusing outright.
fn hits_match_space(hits: &Value, space_ref: &str) -> bool {
    hits.as_array().is_some_and(|hits| {
        hits.iter().all(|hit| {
            hit.get("space_id").and_then(Value::as_str) == Some(space_ref)
                && hit.get("message_id").and_then(Value::as_str).is_some()
                && hit.get("thread_id").and_then(Value::as_str).is_some()
        })
    })
}

/// `POST /api/v1/spaces/{space_ref}/search`.
async fn search_space_messages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Result<Json<SearchBody>, JsonRejection>,
) -> Response {
    if !state.space_features.message_search_api {
        return disabled();
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The search request is invalid.",
            )
        }
    };
    let ValidSearch { mut forward } = match validate_search(body) {
        Ok(valid) => valid,
        Err(response) => return response,
    };
    let (org_id, space_ref) = match authorize_scope(&state, &user, &space_ref).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    // A fresh decision per query, never cached. A decline means "no shared read": the search is
    // forwarded without a decision and Session Core answers owner-only, labelled by `coverage`.
    let (decision, _denial) = match mint_read_decision(&state, &user, &org_id, &space_ref).await {
        Ok(minted) => minted,
        Err(response) => return response,
    };
    match decision {
        Some((decision_ref, token)) => {
            forward.insert("space_read_decision_ref".into(), Value::String(decision_ref));
            forward.insert("space_read_decision_token".into(), Value::String(token));
        }
        None => {
            // Owner-only coverage still needs a LIVE membership: confirm it before forwarding.
            match super::resolve_space_role(&state, &user, &org_id, &space_ref).await {
                Ok(_) => {}
                Err((status, _))
                    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND =>
                {
                    return refuse(
                        StatusCode::FORBIDDEN,
                        "active_membership_required",
                        "An active Space membership is required to search it.",
                    )
                }
                Err(_) => {
                    return refuse(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "space_decision_unavailable",
                        "The Space read decision is unavailable.",
                    )
                }
            }
        }
    }
    let (session_token, model_token) = match delegated_bearers(&state, &user, &headers).await {
        Ok(tokens) => tokens,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/spaces/{}/messages/search",
        state.model_gateway_url,
        urlencoding::encode(&space_ref)
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::POST,
        &url,
        Some(Value::Object(forward)),
        model_token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        tracing::info!(status = status.as_u16(), "space message search refused upstream");
        return map_model_error(status, &response, false);
    }
    let hits = response.get("hits").cloned().unwrap_or(Value::Null);
    let coverage = response.get("coverage").and_then(Value::as_str);
    if !hits_match_space(&hits, &space_ref)
        || !matches!(coverage, Some("shared" | "own_threads_only"))
    {
        tracing::warn!("space message search response failed the hit-space assertion");
        return bad_gateway();
    }
    tracing::info!(
        hits = hits.as_array().map_or(0, Vec::len),
        coverage = coverage.unwrap_or_default(),
        "space message search served"
    );
    reply(
        StatusCode::OK,
        json!({ "data": {
            "hits": hits,
            "next_cursor": response.get("next_cursor").cloned().filter(Value::is_string).unwrap_or(Value::Null),
            "truncated": response.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            "coverage": coverage,
            "excluded": response
                .get("excluded")
                .cloned()
                .filter(Value::is_array)
                .unwrap_or_else(|| json!([])),
        }}),
    )
}

/// `GET /api/v1/spaces/{space_ref}/entries/{message_id}`.
async fn resolve_space_entry(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, message_id)): Path<(String, String)>,
) -> Response {
    if !state.space_features.message_search_api {
        return disabled();
    }
    let message_id = message_id.trim().to_owned();
    // A malformed id is the same answer as a missing one: the route never says which.
    if !valid_message_id(&message_id) {
        return entry_not_found();
    }
    let (org_id, space_ref) = match authorize_scope(&state, &user, &space_ref).await {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    let (decision, denial_reason) =
        match mint_read_decision(&state, &user, &org_id, &space_ref).await {
            Ok(minted) => minted,
            Err(response) => return response,
        };
    // Addressing another member's turn with no read authority must not silently succeed, so a
    // decline is a 403 here (as on the transcript route), unlike search's owner-only fallback.
    let Some((decision_ref, token)) = decision else {
        let message = "Reading this Space's posts is not authorized.";
        return match denial_reason {
            Some(reason) => reply(
                StatusCode::FORBIDDEN,
                error_with_details(denial::OP_READ, message, reason.details_value()),
            ),
            None => refuse(StatusCode::FORBIDDEN, "space_read_not_authorized", message),
        };
    };
    let (session_token, model_token) = match delegated_bearers(&state, &user, &headers).await {
        Ok(tokens) => tokens,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/spaces/{}/messages/resolve",
        state.model_gateway_url,
        urlencoding::encode(&space_ref)
    );
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::POST,
        &url,
        Some(json!({
            "message_id": message_id,
            "space_read_decision_ref": decision_ref,
            "space_read_decision_token": token,
        })),
        model_token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return map_model_error(status, &response, true);
    }
    let thread_id = response.get("thread_id").and_then(Value::as_str);
    let resolved_id = response.get("message_id").and_then(Value::as_str);
    let same_space = response.get("space_id").and_then(Value::as_str) == Some(space_ref.as_str());
    match (thread_id, resolved_id, same_space) {
        (Some(thread_id), Some(resolved_id), true) if resolved_id == message_id => reply(
            StatusCode::OK,
            json!({ "data": {
                "kind": "message",
                "space_ref": space_ref,
                "thread_id": thread_id,
                "message_id": resolved_id,
            }}),
        ),
        _ => bad_gateway(),
    }
}

/// `GET /api/v1/spaces/features`: shell-level rollout flags for places with no Space in scope
/// (the command palette). Presentation only, never an authorization input.
async fn space_features(State(state): State<AppState>) -> Response {
    let flags = &state.space_features;
    reply(
        StatusCode::OK,
        json!({ "data": { "features": {
            "message_search": flags.message_search_api && flags.message_search_ui,
            "entry_links": flags.entry_links_ui,
            "command_palette": flags.command_palette_ui,
            "room_pins": flags.room_pins,
            "thread_fork": flags.thread_fork_api,
        }}}),
    )
}
