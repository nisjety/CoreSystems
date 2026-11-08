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
            "attention": flags.attention_api && flags.attention_ui,
        }}}),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        clean_id, hits_match_space, map_model_error, upstream_code, valid_message_id,
        validate_search, SearchBody, QUERY_MAX_CHARS,
    };
    use crate::config::AppState;
    use axum::{
        body::Body,
        http::{header, HeaderMap, Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_string_contains, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SPACE: &str = "space-1";

    fn search_body(value: Value) -> SearchBody {
        serde_json::from_value(value).expect("a SearchBody")
    }

    fn refusal(result: Result<super::ValidSearch, axum::response::Response>) -> (u16, String) {
        match result {
            Ok(_) => panic!("expected a refusal"),
            Err(response) => {
                let status = response.status().as_u16();
                let bytes = futures_util::FutureExt::now_or_never(response.into_body().collect())
                    .expect("a buffered body")
                    .unwrap()
                    .to_bytes();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                (status, body["error"]["code"].as_str().unwrap_or_default().to_owned())
            }
        }
    }

    // ----- pure validation -------------------------------------------------------------------

    #[test]
    fn query_length_is_2_to_200_with_invalid_query_details() {
        for bad in ["", " ", "a", " a ", &"a".repeat(QUERY_MAX_CHARS + 1), "ab\ncd"] {
            let (status, code) = refusal(validate_search(search_body(json!({ "query": bad }))));
            assert_eq!((status, code.as_str()), (400, "invalid_query"), "{bad:?}");
        }
        for ok in ["ab", &"a".repeat(QUERY_MAX_CHARS), "  trimmed  ", "\u{00e5}\u{00e5}"] {
            assert!(validate_search(search_body(json!({ "query": ok }))).is_ok(), "{ok:?}");
        }
        // Characters, not bytes.
        assert!(validate_search(search_body(json!({ "query": "\u{00e5}".repeat(200) }))).is_ok());
        // The details name the bounds so the SPA can word the hint.
        let response = match validate_search(search_body(json!({ "query": "a" }))) {
            Err(response) => response,
            Ok(_) => panic!("refused"),
        };
        let bytes = futures_util::FutureExt::now_or_never(response.into_body().collect())
            .unwrap()
            .unwrap()
            .to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["details"], json!({ "min_chars": 2, "max_chars": 200 }));
        let forwarded = validate_search(search_body(json!({ "query": "  trimmed  " }))).ok().unwrap();
        assert_eq!(forwarded.forward["query"], "trimmed");
    }

    #[test]
    fn scope_decides_which_filter_is_forwarded() {
        let forward = |body: Value| validate_search(search_body(body)).ok().unwrap().forward;
        let room = forward(json!({
            "query": "plan", "thread_id": "t-1", "author_subject_id": "u-1"
        }));
        assert_eq!(room["scope"], "room");
        assert!(room.get("thread_id").is_none() && room.get("author_subject_id").is_none());
        let thread = forward(json!({
            "query": "plan", "scope": "thread", "thread_id": "t-1", "author_subject_id": "u-1"
        }));
        assert_eq!(thread["thread_id"], "t-1");
        assert!(thread.get("author_subject_id").is_none());
        let person = forward(json!({
            "query": "plan", "scope": "person", "thread_id": "t-1", "author_subject_id": "u-1"
        }));
        assert_eq!(person["author_subject_id"], "u-1");
        assert!(person.get("thread_id").is_none());
        // A scope without its id is refused, as is an unknown scope.
        for body in [
            json!({ "query": "plan", "scope": "thread" }),
            json!({ "query": "plan", "scope": "person" }),
            json!({ "query": "plan", "scope": "everywhere" }),
            json!({ "query": "plan", "scope": "thread", "thread_id": "a\nb" }),
            json!({ "query": "plan", "scope": "thread", "thread_id": "x".repeat(201) }),
        ] {
            let (status, code) = refusal(validate_search(search_body(body.clone())));
            assert_eq!((status, code.as_str()), (400, "invalid_filter"), "{body}");
        }
    }

    #[test]
    fn role_sort_time_cursor_and_limit_are_validated_and_forwarded() {
        let forward = |body: Value| validate_search(search_body(body)).ok().unwrap().forward;
        let all = forward(json!({
            "query": "plan", "prefix": true, "role": "assistant", "sort": "relevance",
            "since": "2026-10-01T00:00:00Z", "until": "2026-11-01T00:00:00Z",
            "cursor": "abc", "limit": 25
        }));
        assert_eq!(all["prefix"], true);
        assert_eq!(all["role"], "assistant");
        assert_eq!(all["sort"], "relevance");
        assert_eq!(all["since"], "2026-10-01T00:00:00Z");
        assert_eq!(all["cursor"], "abc");
        assert_eq!(all["limit"], 25);
        let minimal = forward(json!({ "query": "plan" }));
        assert_eq!(minimal["prefix"], false);
        for key in ["role", "sort", "since", "until", "cursor", "limit"] {
            assert!(minimal.get(key).is_none(), "{key}");
        }
        for (body, code) in [
            (json!({ "query": "plan", "role": "system" }), "invalid_filter"),
            (json!({ "query": "plan", "sort": "oldest" }), "invalid_filter"),
            (json!({ "query": "plan", "since": "x".repeat(65) }), "invalid_filter"),
            (json!({ "query": "plan", "until": "a\nb" }), "invalid_filter"),
            (json!({ "query": "plan", "cursor": "c".repeat(2049) }), "invalid_cursor"),
            (json!({ "query": "plan", "cursor": "a\nb" }), "invalid_cursor"),
        ] {
            let (status, got) = refusal(validate_search(search_body(body.clone())));
            assert_eq!((status, got.as_str()), (400, code), "{body}");
        }
    }

    #[test]
    fn id_helpers_and_the_hit_space_assertion() {
        assert_eq!(clean_id("  a-1 "), Some("a-1".to_owned()));
        assert_eq!(clean_id(""), None);
        assert_eq!(clean_id("a\u{0007}b"), None);
        assert_eq!(clean_id(&"a".repeat(201)), None);
        assert!(valid_message_id("msg_1-A"));
        assert!(!valid_message_id(""));
        assert!(!valid_message_id("a/b"));
        assert!(!valid_message_id("a b"));
        assert!(!valid_message_id(&"a".repeat(65)));
        let hit = |space: &str| json!({ "space_id": space, "message_id": "m", "thread_id": "t" });
        assert!(hits_match_space(&json!([hit(SPACE), hit(SPACE)]), SPACE));
        assert!(hits_match_space(&json!([]), SPACE));
        assert!(!hits_match_space(&json!([hit(SPACE), hit("space-2")]), SPACE));
        assert!(!hits_match_space(&json!([{ "space_id": SPACE, "thread_id": "t" }]), SPACE));
        assert!(!hits_match_space(&Value::Null, SPACE));
    }

    #[test]
    fn model_gateway_error_map() {
        let body = |code: &str| json!({ "error": { "code": code, "message": "upstream text" } });
        let status_and_code = |status: StatusCode, payload: Value, entries: bool| {
            let response = map_model_error(status, &payload, entries);
            let bytes = futures_util::FutureExt::now_or_never(response.into_body().collect())
                .unwrap()
                .unwrap()
                .to_bytes();
            let out: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(!out.to_string().contains("upstream text"), "no upstream text crosses");
            (
                out["error"]["code"].as_str().unwrap().to_owned(),
            )
        };
        let cases: Vec<(StatusCode, Value, bool, &str)> = vec![
            (StatusCode::NOT_FOUND, body("entry_not_found"), false, "entry_not_found"),
            (StatusCode::NOT_FOUND, json!({}), false, "space_search_disabled"),
            (StatusCode::NOT_FOUND, json!({}), true, "entry_not_found"),
            (StatusCode::NOT_FOUND, body("space_search_disabled"), true, "space_search_disabled"),
            (StatusCode::BAD_REQUEST, body("invalid_cursor"), false, "invalid_cursor"),
            (StatusCode::BAD_REQUEST, body("invalid_filter"), false, "invalid_filter"),
            (StatusCode::BAD_REQUEST, body("invalid_query"), false, "invalid_query"),
            (StatusCode::BAD_REQUEST, body("something_else"), false, "invalid_model_gateway_response"),
            (StatusCode::UNAUTHORIZED, json!({}), false, "delegated_auth_unavailable"),
            (StatusCode::FORBIDDEN, body("space_read_not_authorized"), false, "space_read_not_authorized"),
            (StatusCode::TOO_MANY_REQUESTS, json!({}), false, "search_busy"),
            (StatusCode::GATEWAY_TIMEOUT, json!({}), false, "search_timeout"),
            (StatusCode::SERVICE_UNAVAILABLE, body("search_timeout"), false, "search_timeout"),
            (StatusCode::SERVICE_UNAVAILABLE, body("search_unavailable"), false, "search_unavailable"),
            (StatusCode::INTERNAL_SERVER_ERROR, json!("html"), false, "search_unavailable"),
            (StatusCode::IM_A_TEAPOT, json!({}), false, "invalid_model_gateway_response"),
        ];
        for (status, payload, entries, expected) in cases {
            assert_eq!(status_and_code(status, payload, entries).0, expected, "{status}");
        }
        assert_eq!(upstream_code(&body("search_busy")), Some("search_busy"));
        assert_eq!(upstream_code(&body("unknown_word")), None);
        let teapot = map_model_error(StatusCode::TOO_MANY_REQUESTS, &json!({}), false);
        assert_eq!(teapot.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // ----- routes, through the real router ---------------------------------------------------

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

    fn hit(space: &str) -> Value {
        json!({ "space_id": space, "thread_id": "t-1", "message_id": "m-1", "snippet": "plan" })
    }

    fn search_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "hits": [hit(SPACE)], "next_cursor": "n-1", "truncated": false,
            "coverage": "shared", "excluded": []
        }))
    }

    async fn fixture(search: ResponseTemplate, flag: bool) -> Fx {
        let server = MockServer::start().await;
        mount(
            &server,
            "GET",
            "/api/auth/get-session",
            ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "search-user", "email": "s@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })),
        )
        .await;
        mount(
            &server,
            "GET",
            "/api/v1/me/session-context",
            ResponseTemplate::new(200).set_body_json(json!({
                "userId": "search-user", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })),
        )
        .await;
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
            "/api/session-core/token",
            ResponseTemplate::new(200).set_body_json(json!({ "token": "session-test-token" })),
        )
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
            "POST",
            "/api/v1/internal/spaces/thread-read-decision",
            ResponseTemplate::new(200).set_body_json(json!({
                "data": { "decision": { "decision_ref": "dec-1" }, "token": "dec-token" }
            })),
        )
        .await;
        mount(&server, "POST", &format!("/v1/spaces/{SPACE}/messages/search"), search).await;
        let mut state = crate::tests::test_state(false);
        let uri = server.uri();
        state.auth_core_url = uri.clone();
        state.user_core_url = uri.clone();
        state.model_gateway_url = uri.clone();
        state.application_convex_url = uri;
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.message_search_api = flag;
        Fx { server, state }
    }

    async fn send(fx: &Fx, method: &str, uri: &str, body: Option<String>) -> (u16, HeaderMap, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(request.body(Body::from(body.unwrap_or_default())).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn search(fx: &Fx, body: Value) -> (u16, HeaderMap, Value) {
        send(fx, "POST", &format!("/api/v1/spaces/{SPACE}/search"), Some(body.to_string())).await
    }

    async fn requests_to(server: &MockServer, needle: &str) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path().contains(needle))
            .collect()
    }

    #[tokio::test]
    async fn search_flag_off_is_404_space_search_disabled_before_any_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), false).await;
        let (status, headers, body) = search(&fx, json!({ "query": "plan" })).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "space_search_disabled");
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert!(requests_to(&fx.server, "thread-read-decision").await.is_empty());
    }

    #[tokio::test]
    async fn search_mints_fresh_decision_per_query() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        for query in ["plan", "budget"] {
            let (status, _, body) = search(&fx, json!({ "query": query })).await;
            assert_eq!(status, 200);
            assert_eq!(body["data"]["coverage"], "shared");
        }
        let decisions = requests_to(&fx.server, "thread-read-decision").await;
        assert_eq!(decisions.len(), 2, "one decision per query, never reused");
        let keys: Vec<String> = decisions
            .iter()
            .map(|request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                body["idempotency_key"].as_str().unwrap().to_owned()
            })
            .collect();
        assert_ne!(keys[0], keys[1]);
        let searches = requests_to(&fx.server, "messages/search").await;
        assert_eq!(searches.len(), 2);
        for sent in &searches {
            let body: Value = serde_json::from_slice(&sent.body).unwrap();
            assert_eq!(body["space_read_decision_ref"], "dec-1");
            assert_eq!(body["space_read_decision_token"], "dec-token");
            assert_eq!(body["scope"], "room");
        }
    }

    #[tokio::test]
    async fn no_store_header_and_no_cache_use() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        let (status, headers, body) = search(&fx, json!({ "query": "secret-query-text" })).await;
        assert_eq!(status, 200);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(body["data"]["hits"].as_array().unwrap().len(), 1);
        assert_eq!(body["data"]["next_cursor"], "n-1");
        assert_eq!(body["data"]["truncated"], false);
        // The query text travels in a POST body only, never in any URL.
        for request in fx.server.received_requests().await.unwrap() {
            assert!(
                !request.url.to_string().contains("secret-query-text"),
                "query leaked into {}",
                request.url
            );
        }
        // Errors are no-store too.
        let (status, headers, _) = search(&fx, json!({ "query": "a" })).await;
        assert_eq!(status, 400);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    }

    #[tokio::test]
    async fn strict_body_rejects_unknown_fields() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        for extra in [
            ("space_read_decision_token", json!("forged")),
            ("org_id", json!("org-2")),
            ("user_id", json!("u-2")),
            ("space_id", json!("space-2")),
        ] {
            let mut body = json!({ "query": "plan" });
            body[extra.0] = extra.1;
            let (status, _, out) = search(&fx, body).await;
            assert_eq!(status, 400, "{}", extra.0);
            assert_eq!(out["error"]["code"], "invalid_request");
        }
        let (status, _, out) = send(
            &fx,
            "POST",
            &format!("/api/v1/spaces/{SPACE}/search"),
            Some("not json".to_owned()),
        )
        .await;
        assert_eq!((status, out["error"]["code"].clone()), (400, json!("invalid_request")));
        assert!(requests_to(&fx.server, "messages/search").await.is_empty());
        assert!(requests_to(&fx.server, "thread-read-decision").await.is_empty());
    }

    #[tokio::test]
    async fn invalid_query_is_400_before_any_decision_is_minted() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        let (status, _, body) = search(&fx, json!({ "query": "x" })).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], "invalid_query");
        assert_eq!(body["error"]["details"], json!({ "min_chars": 2, "max_chars": 200 }));
        assert!(requests_to(&fx.server, "thread-read-decision").await.is_empty());
    }

    #[tokio::test]
    async fn control_unreachable_is_503_not_owner_only() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        fx.server.reset().await;
        let fx = rebuild_after_reset(fx, |server| async move {
            mount(
                &server,
                "POST",
                "/api/v1/internal/spaces/thread-read-decision",
                ResponseTemplate::new(500).set_body_string("down"),
            )
            .await;
            server
        })
        .await;
        let (status, _, body) = search(&fx, json!({ "query": "plan" })).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "space_decision_unavailable");
        assert!(requests_to(&fx.server, "messages/search").await.is_empty(), "never forwarded owner-only");
    }

    /// `reset()` drops every mock; put back the ones every search needs, then let the test add its own.
    async fn rebuild_after_reset<F, Fut>(fx: Fx, extra: F) -> Fx
    where
        F: FnOnce(MockServer) -> Fut,
        Fut: std::future::Future<Output = MockServer>,
    {
        let Fx { server, state } = fx;
        mount(
            &server,
            "GET",
            "/api/auth/get-session",
            ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "search-user", "email": "s@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })),
        )
        .await;
        mount(
            &server,
            "GET",
            "/api/v1/me/session-context",
            ResponseTemplate::new(200).set_body_json(json!({
                "userId": "search-user", "orgId": "org-1", "role": "member",
                "onboardingStatus": "COMPLETED"
            })),
        )
        .await;
        mount(
            &server,
            "POST",
            "/api/query",
            ResponseTemplate::new(200).set_body_json(json!({
                "value": [{ "spaceRef": SPACE, "lifecycle": "active" }]
            })),
        )
        .await;
        mount(
            &server,
            "GET",
            "/api/session-core/token",
            ResponseTemplate::new(200).set_body_json(json!({ "token": "session-test-token" })),
        )
        .await;
        let server = extra(server).await;
        Fx { server, state }
    }

    #[tokio::test]
    async fn none_decision_is_forwarded_as_no_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        fx.server.reset().await;
        let fx = rebuild_after_reset(fx, |server| async move {
            mount(
                &server,
                "POST",
                "/api/v1/internal/spaces/thread-read-decision",
                ResponseTemplate::new(403).set_body_json(json!({
                    "error": "No.", "code": "space_entitlement_off", "resolvable_by": "org_admin"
                })),
            )
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
                "POST",
                &format!("/v1/spaces/{SPACE}/messages/search"),
                ResponseTemplate::new(200).set_body_json(json!({
                    "hits": [hit(SPACE)], "coverage": "own_threads_only"
                })),
            )
            .await;
            server
        })
        .await;
        let (status, _, body) = search(&fx, json!({ "query": "plan" })).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["coverage"], "own_threads_only");
        let sent = requests_to(&fx.server, "messages/search").await;
        assert_eq!(sent.len(), 1);
        let forwarded: Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert!(forwarded.get("space_read_decision_ref").is_none());
        assert!(forwarded.get("space_read_decision_token").is_none());
        // The live membership was confirmed first.
        assert_eq!(requests_to(&fx.server, "space-1/membership").await.len(), 1);
    }

    #[tokio::test]
    async fn a_declined_decision_without_live_membership_is_403_and_nothing_is_forwarded() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (membership, expected) in [(403u16, 403u16), (404, 403), (500, 503)] {
            let fx = fixture(search_ok(), true).await;
            fx.server.reset().await;
            let fx = rebuild_after_reset(fx, |server| async move {
                mount(
                    &server,
                    "POST",
                    "/api/v1/internal/spaces/thread-read-decision",
                    ResponseTemplate::new(403).set_body_json(json!({ "code": "space_entitlement_off" })),
                )
                .await;
                mount(
                    &server,
                    "GET",
                    &format!("/api/v1/internal/spaces/{SPACE}/membership"),
                    ResponseTemplate::new(membership).set_body_string("no"),
                )
                .await;
                server
            })
            .await;
            let (status, _, body) = search(&fx, json!({ "query": "plan" })).await;
            assert_eq!(status, expected, "membership {membership}");
            let code = if expected == 403 { "active_membership_required" } else { "space_decision_unavailable" };
            assert_eq!(body["error"]["code"], code);
            assert!(requests_to(&fx.server, "messages/search").await.is_empty());
        }
    }

    #[tokio::test]
    async fn hit_from_other_space_is_502() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for payload in [
            json!({ "hits": [hit(SPACE), hit("space-other")], "coverage": "shared" }),
            json!({ "hits": [hit(SPACE)], "coverage": "everything" }),
            json!({ "hits": "nope", "coverage": "shared" }),
            json!({ "coverage": "shared" }),
        ] {
            let fx = fixture(ResponseTemplate::new(200).set_body_json(payload.clone()), true).await;
            let (status, _, body) = search(&fx, json!({ "query": "plan" })).await;
            assert_eq!(status, 502, "{payload}");
            assert_eq!(body["error"]["code"], "invalid_model_gateway_response");
            assert!(body.get("data").is_none(), "no partial hits are returned");
        }
    }

    #[tokio::test]
    async fn model_gateway_refusals_are_mapped_through_the_route() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (upstream, status, code) in [
            (429u16, 429u16, "search_busy"),
            (504, 503, "search_timeout"),
            (500, 503, "search_unavailable"),
            (403, 403, "space_read_not_authorized"),
            (404, 404, "space_search_disabled"),
        ] {
            let fx = fixture(ResponseTemplate::new(upstream).set_body_json(json!({})), true).await;
            let (got, headers, body) = search(&fx, json!({ "query": "plan" })).await;
            assert_eq!((got, body["error"]["code"].as_str().unwrap()), (status, code));
            assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        }
    }

    // ----- entries and features --------------------------------------------------------------

    fn entry_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "space_id": SPACE, "thread_id": "t-1", "message_id": "m-1"
        }))
    }

    async fn mount_resolve(fx: &Fx, template: ResponseTemplate) {
        mount(&fx.server, "POST", &format!("/v1/spaces/{SPACE}/messages/resolve"), template).await;
    }

    #[tokio::test]
    async fn entries_route_resolves_a_message_address_under_a_fresh_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        mount_resolve(&fx, entry_ok()).await;
        let (status, headers, body) =
            send(&fx, "GET", &format!("/api/v1/spaces/{SPACE}/entries/m-1"), None).await;
        assert_eq!(status, 200);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(
            body["data"],
            json!({ "kind": "message", "space_ref": SPACE, "thread_id": "t-1", "message_id": "m-1" })
        );
        let sent = requests_to(&fx.server, "messages/resolve").await;
        let forwarded: Value = serde_json::from_slice(&sent[0].body).unwrap();
        assert_eq!(forwarded["message_id"], "m-1");
        assert_eq!(forwarded["space_read_decision_ref"], "dec-1");
        assert_eq!(forwarded["space_read_decision_token"], "dec-token");
    }

    #[tokio::test]
    async fn entries_route_returns_403_when_no_decision() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        fx.server.reset().await;
        let mut fx = rebuild_after_reset(fx, |server| async move {
            mount(
                &server,
                "POST",
                "/api/v1/internal/spaces/thread-read-decision",
                ResponseTemplate::new(403).set_body_json(json!({
                    "error": "No.", "code": "space_membership_required", "resolvable_by": "none"
                })),
            )
            .await;
            server
        })
        .await;
        let uri = format!("/api/v1/spaces/{SPACE}/entries/m-1");
        let (status, _, body) = send(&fx, "GET", &uri, None).await;
        assert_eq!(status, 403, "unlike search there is no owner-only fallback");
        assert_eq!(body["error"]["code"], "space_read_not_authorized");
        // With typed denials on, the reason is carried.
        fx.state.space_features.denials_typed = true;
        let (status, _, body) = send(&fx, "GET", &uri, None).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["details"]["reason"], "space_membership_required");
        assert!(requests_to(&fx.server, "messages/resolve").await.is_empty());
    }

    #[tokio::test]
    async fn entries_route_hides_malformed_missing_and_foreign_messages_alike() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fixture(search_ok(), true).await;
        // A malformed id never reaches a decision.
        let (status, _, body) =
            send(&fx, "GET", &format!("/api/v1/spaces/{SPACE}/entries/bad%20id"), None).await;
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("entry_not_found")));
        assert!(requests_to(&fx.server, "thread-read-decision").await.is_empty());
        // A missing message (model-gateway 404 without a code) and an unrecognised answer.
        mount_resolve(&fx, ResponseTemplate::new(404).set_body_json(json!({}))).await;
        let (status, _, body) =
            send(&fx, "GET", &format!("/api/v1/spaces/{SPACE}/entries/m-1"), None).await;
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("entry_not_found")));
        // A resolved message that is another Space's, or another id, is a contract failure.
        for payload in [
            json!({ "space_id": "space-other", "thread_id": "t-1", "message_id": "m-1" }),
            json!({ "space_id": SPACE, "thread_id": "t-1", "message_id": "m-2" }),
            json!({ "space_id": SPACE, "message_id": "m-1" }),
        ] {
            let fx = fixture(search_ok(), true).await;
            mount_resolve(&fx, ResponseTemplate::new(200).set_body_json(payload.clone())).await;
            let (status, _, body) =
                send(&fx, "GET", &format!("/api/v1/spaces/{SPACE}/entries/m-1"), None).await;
            assert_eq!(status, 502, "{payload}");
            assert_eq!(body["error"]["code"], "invalid_model_gateway_response");
        }
        // Flag off.
        let off = fixture(search_ok(), false).await;
        let (status, _, body) =
            send(&off, "GET", &format!("/api/v1/spaces/{SPACE}/entries/m-1"), None).await;
        assert_eq!((status, body["error"]["code"].clone()), (404, json!("space_search_disabled")));
    }

    #[tokio::test]
    async fn features_route_is_registered_before_space_ref() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fixture(search_ok(), false).await;
        let (status, headers, body) = send(&fx, "GET", "/api/v1/spaces/features", None).await;
        assert_eq!(status, 200, "a room can never shadow the static segment");
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(
            body["data"]["features"],
            json!({
                "message_search": false, "entry_links": false, "command_palette": false,
                "room_pins": false, "thread_fork": false, "attention": false
            })
        );
        fx.state.space_features.message_search_api = true;
        let (_, _, body) = send(&fx, "GET", "/api/v1/spaces/features", None).await;
        assert_eq!(body["data"]["features"]["message_search"], false, "needs the UI flag too");
        fx.state.space_features.message_search_ui = true;
        fx.state.space_features.entry_links_ui = true;
        fx.state.space_features.command_palette_ui = true;
        fx.state.space_features.thread_fork_api = true;
        fx.state.space_features.room_pins = true;
        let (_, _, body) = send(&fx, "GET", "/api/v1/spaces/features", None).await;
        let features = &body["data"]["features"];
        for key in ["message_search", "entry_links", "command_palette", "thread_fork", "room_pins"] {
            assert_eq!(features[key], true, "{key}");
        }
        assert_eq!(features["attention"], false, "needs both attention flags");
    }
}
