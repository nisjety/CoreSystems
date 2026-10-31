//! `GET /v1/threads/search`: conversation search for the chat page (build plan WS17 S3).
//!
//! The gateway forwards the verified caller to Session Core's `MessageSearch` and answers
//! for the owner only. The purpose is always UI, and the Space decision fields are not
//! read from the query, so a browser cannot widen a search past the owner's own threads.
//! The recall tool reaches the same RPC in-process with the RECALL purpose; it does not
//! use this route.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use mp_contracts::model_plane::v1::{ArchiveFilter, SearchMessagesRequest, SearchPurpose};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{
    authenticated_session_request, session_thread_error, timestamp_to_rfc3339, HttpJsonError,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct SearchThreadsQuery {
    /// What the person typed. Empty answers an empty page.
    q: Option<String>,
    /// Restricts the search to one thread, one hit per message.
    thread_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
    /// The surface that created the thread; the chat page passes "chat".
    origin: Option<String>,
    /// "active" (the default), "archived" or "all".
    archived: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchThreadsResponse {
    hits: Vec<SearchHitResponse>,
    /// Omitted on the last page.
    #[serde(skip_serializing_if = "String::is_empty")]
    next_cursor: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchHitResponse {
    thread_id: String,
    message_id: String,
    role: String,
    /// The matched text with each matched word wrapped in U+E000 and U+E001.
    snippet: String,
    created_at: String,
    thread_title: String,
    /// Omitted for a thread that is not in a Space.
    #[serde(skip_serializing_if = "String::is_empty")]
    space_id: String,
    archived: bool,
}

/// The archive state a query names. An absent state is the active threads, as the listing
/// is without it.
fn archive_filter(raw: Option<&str>) -> Result<ArchiveFilter, HttpJsonError> {
    match raw.unwrap_or("active") {
        "active" => Ok(ArchiveFilter::Unspecified),
        "archived" => Ok(ArchiveFilter::Archived),
        "all" => Ok(ArchiveFilter::All),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "archived must be active, archived or all" })),
        )),
    }
}

/// Maps the typed search statuses to the codes the BFF and the page read. A disabled
/// deployment and a timed-out search are both retryable 503s; the page then falls back to
/// its local filter.
fn search_error(context: &str, error: &tonic::Status) -> HttpJsonError {
    match (error.code(), error.message()) {
        (tonic::Code::Unavailable, "message_search_disabled") => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "history_search_disabled",
                    "message": "conversation search is not enabled on this deployment",
                }
            })),
        ),
        (tonic::Code::Unavailable, "message_search_timeout") => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "history_search_timeout",
                    "message": "conversation search took too long; try a shorter query",
                }
            })),
        ),
        _ => session_thread_error(context, error),
    }
}

pub(crate) async fn search_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Query(query): Query<SearchThreadsQuery>,
) -> Result<Json<SearchThreadsResponse>, HttpJsonError> {
    let archived = archive_filter(query.archived.as_deref())?;
    let response = state
        .message_search_client
        .clone()
        .search_messages(authenticated_session_request(
            SearchMessagesRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                query: query.q.unwrap_or_default(),
                limit: query.limit.unwrap_or(0),
                cursor: query.cursor.unwrap_or_default(),
                origin: query.origin.unwrap_or_default(),
                thread_id: query.thread_id.unwrap_or_default(),
                purpose: SearchPurpose::Ui as i32,
                archived: archived as i32,
                ..Default::default()
            },
            &bearer,
        )?)
        .await
        .map_err(|error| search_error("session-core search_messages failed", &error))?
        .into_inner();

    let hits = response
        .hits
        .into_iter()
        .map(|hit| SearchHitResponse {
            thread_id: hit.thread_id,
            message_id: hit.message_id,
            role: hit.role,
            snippet: hit.snippet,
            created_at: timestamp_to_rfc3339(hit.created_at),
            thread_title: hit.thread_title,
            space_id: hit.space_id,
            archived: hit.archived,
        })
        .collect();
    Ok(Json(SearchThreadsResponse {
        hits,
        next_cursor: response.next_cursor,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_state_defaults_to_active_and_rejects_anything_else() {
        assert_eq!(archive_filter(None).unwrap(), ArchiveFilter::Unspecified);
        assert_eq!(
            archive_filter(Some("active")).unwrap(),
            ArchiveFilter::Unspecified
        );
        assert_eq!(
            archive_filter(Some("archived")).unwrap(),
            ArchiveFilter::Archived
        );
        assert_eq!(archive_filter(Some("all")).unwrap(), ArchiveFilter::All);
        let (status, _) = archive_filter(Some("both")).expect_err("unknown state");
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_disabled_deployment_answers_a_typed_503_the_page_can_recognise() {
        let error = tonic::Status::unavailable("message_search_disabled");
        let (status, Json(body)) = search_error("ctx", &error);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "history_search_disabled");
    }

    #[test]
    fn a_timed_out_search_is_a_retryable_503_not_an_internal_error() {
        let error = tonic::Status::unavailable("message_search_timeout");
        let (status, Json(body)) = search_error("ctx", &error);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "history_search_timeout");
    }

    #[test]
    fn a_malformed_search_request_stays_a_400() {
        let error = tonic::Status::invalid_argument("cursor is not a message search cursor");
        let (status, _) = search_error("ctx", &error);
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn the_last_page_omits_the_cursor_and_a_personal_hit_omits_the_space() {
        let page = SearchThreadsResponse {
            hits: vec![SearchHitResponse {
                thread_id: "t1".to_owned(),
                message_id: "m1".to_owned(),
                role: "user".to_owned(),
                snippet: "a \u{E000}budget\u{E001} line".to_owned(),
                created_at: "2026-10-01T00:00:00+00:00".to_owned(),
                thread_title: "Trip".to_owned(),
                space_id: String::new(),
                archived: false,
            }],
            next_cursor: String::new(),
        };
        let value = serde_json::to_value(&page).unwrap();
        assert!(value.get("next_cursor").is_none());
        assert!(value["hits"][0].get("space_id").is_none());
        assert_eq!(value["hits"][0]["snippet"], "a \u{E000}budget\u{E001} line");
    }
}
