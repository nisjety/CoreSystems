//! Browser omnibox actions (WS9.A, D9 S1): `browser_omnibox.search` and
//! `browser_omnibox.suggest` on the generic action surface.
//!
//! Both are thin wrappers over the search handlers in `domains::search`, so the
//! zero-data-retention gate, the org-scoped cache admission and the upstream
//! calls stay where they already are. They are human-only: neither id is in the
//! Model Plane's eligibility list (`model-eligibility.ts`).

use std::collections::HashMap;

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::search::{search_suggestions, search_web}, envelope::error,
    middleware::AuthenticatedUser,
};

use super::dispatchers::owner_json_response_to_envelope;

/// Longest free-text query the search action accepts, in characters.
const SEARCH_QUERY_MAX_CHARS: usize = 500;
/// Longest prefix the suggest action accepts, in characters.
const SUGGEST_QUERY_MAX_CHARS: usize = 200;
/// Result rows the search action may ask for (the registry input caps it too).
const SEARCH_LIMIT_MAX: u64 = 10;
/// Suggestion rows the dropdown asks for (history rows and the Ask row are local).
const SUGGEST_LIMIT: &str = "6";

/// The trimmed `query` field, or None when it is missing, empty, too long or
/// carries a control character. A control character is never a legitimate part
/// of a search the omnibox types.
fn query_of(input: &Value, max_chars: usize) -> Option<String> {
    let query = input.get("query").and_then(Value::as_str)?.trim();
    if query.is_empty() || query.chars().count() > max_chars || query.chars().any(char::is_control) {
        return None;
    }
    Some(query.to_owned())
}

fn invalid_input(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(error("invalid_input", message))).into_response()
}

/// `browser_omnibox.search` with `{ query, limit? }`: the same search the
/// omnibox runs when Enter is pressed on free text.
pub(super) async fn dispatch_browser_omnibox_search(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let Some(query) = query_of(input, SEARCH_QUERY_MAX_CHARS) else {
        return invalid_input(
            "browser_omnibox.search requires a non-empty 'query' of at most 500 characters",
        );
    };
    let limit = input
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, SEARCH_LIMIT_MAX);
    let body = json!({ "query": query, "limit": limit });
    let (status, Json(payload)) = search_web(
        State(state.clone()),
        Extension(user.clone()),
        headers.clone(),
        Json(body),
    )
    .await;
    owner_json_response_to_envelope("browser_omnibox.search", user, status, payload)
}

/// `browser_omnibox.suggest` with `{ query }`: query suggestions for the
/// dropdown. Under ZDR the search handler answers with an empty list.
pub(super) async fn dispatch_browser_omnibox_suggest(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let Some(query) = query_of(input, SUGGEST_QUERY_MAX_CHARS) else {
        return invalid_input(
            "browser_omnibox.suggest requires a non-empty 'query' of at most 200 characters",
        );
    };
    let params: HashMap<String, String> = HashMap::from([
        ("q".to_owned(), query),
        ("scope".to_owned(), "queries".to_owned()),
        ("limit".to_owned(), SUGGEST_LIMIT.to_owned()),
    ]);
    let (status, Json(payload)) = search_suggestions(
        State(state.clone()),
        Extension(user.clone()),
        headers.clone(),
        Query(params),
    )
    .await;
    owner_json_response_to_envelope("browser_omnibox.suggest", user, status, payload)
}
