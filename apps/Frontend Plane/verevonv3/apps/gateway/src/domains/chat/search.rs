//! `GET /api/v1/chat/threads/search`: full-text search over the caller's chat threads
//! (build plan WS17 S3).
//!
//! The request is relayed to Model Gateway's owner search with the origin fixed to
//! `chat`. Nothing in the browser can name a Space, a decision or a purpose, so the
//! search answers for the caller's own chat threads and nothing else.

use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::{
    config::AppState,
    envelope::{error, ok_with_cursor},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

use super::shared;

/// A search page is a handful of hits; the page never asks for the whole corpus.
const SEARCH_PAGE_SIZE: usize = 20;
const MAX_SEARCH_PAGE_SIZE: usize = 50;

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadSearchQuery {
    q: Option<String>,
    #[serde(alias = "threadId")]
    thread_id: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// One page of hits as the page reads them. Model Gateway answers snake_case; the
/// browser reads camelCase, as the thread list does.
#[derive(Debug, Deserialize)]
struct DurableSearchResponse {
    #[serde(default)]
    hits: Vec<DurableSearchHit>,
    #[serde(default)]
    next_cursor: String,
}

#[derive(Debug, Deserialize)]
struct DurableSearchHit {
    thread_id: String,
    message_id: String,
    role: String,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    thread_title: String,
    #[serde(default)]
    space_id: String,
    #[serde(default)]
    archived: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchHitSummary {
    thread_id: String,
    message_id: String,
    role: String,
    /// Matched text with each matched word between U+E000 and U+E001. The page
    /// splits on those markers into text nodes and never parses it as markup.
    snippet: String,
    created_at: String,
    thread_title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    space_id: String,
    archived: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchPageResponse {
    hits: Vec<SearchHitSummary>,
}

pub(super) async fn search_threads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<ThreadSearchQuery>,
) -> Response {
    if authorized_org_id(&state, &user).await.trim().is_empty() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            Json(error(
                "org_scope_required",
                "An authorized organization scope is required.",
            )),
        )
            .into_response();
    }
    let request = SearchRequest {
        query: query.q.as_deref().map(str::trim).unwrap_or_default(),
        thread_id: non_empty(query.thread_id.as_deref()),
        cursor: non_empty(query.cursor.as_deref()),
        limit: query
            .limit
            .unwrap_or(SEARCH_PAGE_SIZE)
            .clamp(1, MAX_SEARCH_PAGE_SIZE),
    };
    let token = shared::model_token(&state, &user, &headers).await;
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error).into_response(),
    };
    let url = thread_search_url(&state.model_gateway_url, &request);
    let (status, Json(payload)) = shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    let Ok(page) = serde_json::from_value::<DurableSearchResponse>(payload) else {
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(error(
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid search page.",
            )),
        )
            .into_response();
    };
    let next_cursor = page.next_cursor;
    let hits = page
        .hits
        .into_iter()
        .map(|hit| SearchHitSummary {
            thread_id: hit.thread_id,
            message_id: hit.message_id,
            role: hit.role,
            snippet: hit.snippet,
            created_at: hit.created_at,
            thread_title: hit.thread_title,
            space_id: hit.space_id,
            archived: hit.archived,
        })
        .collect();
    Json(ok_with_cursor(SearchPageResponse { hits }, &next_cursor)).into_response()
}

struct SearchRequest<'a> {
    query: &'a str,
    thread_id: Option<&'a str>,
    cursor: Option<&'a str>,
    limit: usize,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The Model Gateway search URL. `origin=chat` is always present: the gateway filters
/// on it, so a search can never reach an Agent Run Console or Support thread.
fn thread_search_url(base: &str, request: &SearchRequest<'_>) -> String {
    let mut params = vec![
        format!("q={}", urlencoding::encode(request.query)),
        format!("limit={}", request.limit),
        "origin=chat".to_owned(),
    ];
    if let Some(thread_id) = request.thread_id {
        params.push(format!("thread_id={}", urlencoding::encode(thread_id)));
    }
    if let Some(cursor) = request.cursor {
        params.push(format!("cursor={}", urlencoding::encode(cursor)));
    }
    format!("{base}/v1/threads/search?{}", params.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(query: &str) -> SearchRequest<'_> {
        SearchRequest {
            query,
            thread_id: None,
            cursor: None,
            limit: SEARCH_PAGE_SIZE,
        }
    }

    #[test]
    fn the_search_url_always_fixes_the_origin_to_chat() {
        let url = thread_search_url("http://mg", &request("budget"));
        assert!(url.starts_with("http://mg/v1/threads/search?"));
        assert!(url.contains("origin=chat"));
        assert!(url.contains("q=budget"));
        assert!(!url.contains("space"), "no Space parameter is ever relayed");
    }

    #[test]
    fn the_query_is_encoded_and_optional_parameters_are_omitted_when_empty() {
        let url = thread_search_url("http://mg", &request("a&b=c"));
        assert!(url.contains("q=a%26b%3Dc"));
        assert!(!url.contains("thread_id="));
        assert!(!url.contains("cursor="));
    }

    #[test]
    fn a_named_thread_and_a_cursor_are_carried_through() {
        let url = thread_search_url(
            "http://mg",
            &SearchRequest {
                query: "x",
                thread_id: Some("thread-1"),
                cursor: Some("abc_-"),
                limit: 5,
            },
        );
        assert!(url.contains("thread_id=thread-1"));
        assert!(url.contains("cursor=abc_-"));
        assert!(url.contains("limit=5"));
    }

    #[test]
    fn a_blank_optional_parameter_counts_as_absent() {
        assert_eq!(non_empty(Some("  ")), None);
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(" t ")), Some("t"));
    }
}
