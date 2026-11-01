//! `GET /v1/spaces/:space_id/artifacts` (build plan WS07-S9): the room's artifact aggregate.
//!
//! A thin relay of Session Core's `SpaceRoomReadService.ListSpaceArtifacts`. Authority is
//! Session Core's: this route forwards the caller's verified bearer plus the Control read
//! decision (`space_read_decision_ref` / `space_read_decision_token`, the same pair
//! `list_threads` takes) and holds none of its own. User principals only (a service principal
//! is refused by `authorize_principal_route` outside its allowlist, and here again below).
//! Summaries only: no artifact content crosses this route. Request URLs are never logged by
//! this handler, and refusals carry fixed words, never the upstream text.
//!
//! Errors, as `{error:{code,message,details}}`: `UNAUTHENTICATED` 401 `unauthorized`,
//! `PERMISSION_DENIED` 403 `space_read_not_authorized`, `INVALID_ARGUMENT` 400 `invalid_request`,
//! anything else 503 `space_artifacts_unavailable`. These are not ZDR refusals (a ZDR caller
//! gets an empty list from Session Core), so the typed catalogue is not used.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{ListSpaceArtifactsRequest, SpaceArtifactSummary};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::authenticated_session_request;
use crate::state::AppState;

const MAX_SPACE_ID_BYTES: usize = 128;

/// The Space artifact aggregate routes.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/spaces/:space_id/artifacts", get(list_space_artifacts))
}

#[derive(Debug, Default, Deserialize)]
struct ListSpaceArtifactsQuery {
    limit: Option<u32>,
    cursor: Option<String>,
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

/// The HTTP form of an upstream refusal. Messages are fixed words, never the upstream text.
fn status_response(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::Unauthenticated => {
            error_response(StatusCode::UNAUTHORIZED, "unauthorized", "authentication required")
        }
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "reading this Space's artifacts is not permitted",
        ),
        tonic::Code::InvalidArgument => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the Space artifact request is invalid",
        ),
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "space_artifacts_unavailable",
            "Space artifacts are unavailable",
        ),
    }
}

fn summary_json(summary: &SpaceArtifactSummary) -> Value {
    let mut item = json!({
        "thread_id": summary.thread_id,
        "message_id": summary.message_id,
        "artifact_id": summary.artifact_id,
        "kind": summary.kind,
        "title": summary.title,
        "version": summary.version,
        "version_count": summary.version_count,
        "size_chars": summary.size_chars,
        "created_at": summary.created_at,
        "space_id": summary.space_id,
    });
    if !summary.agent_name.is_empty() {
        item["agent_name"] = json!(summary.agent_name);
    }
    item
}

async fn list_space_artifacts(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(space_id): Path<String>,
    Query(query): Query<ListSpaceArtifactsQuery>,
) -> Response {
    let space_id = space_id.trim().to_owned();
    if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
        return error_response(StatusCode::BAD_REQUEST, "invalid_request", "space_id is invalid");
    }
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "a user principal is required",
        );
    }
    let request = match authenticated_session_request(
        ListSpaceArtifactsRequest {
            org_id: claims.org_id.clone(),
            space_id,
            limit: query.limit.unwrap_or(0),
            cursor: query.cursor.unwrap_or_default(),
            space_read_decision_ref: query.space_read_decision_ref.unwrap_or_default(),
            space_read_decision_token: query.space_read_decision_token.unwrap_or_default(),
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    let response = match state
        .space_room_read_client
        .clone()
        .list_space_artifacts(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => return status_response(&status),
    };
    let next_cursor = if response.next_cursor.is_empty() {
        Value::Null
    } else {
        json!(response.next_cursor)
    };
    Json(json!({
        "artifacts": response.artifacts.iter().map(summary_json).collect::<Vec<_>>(),
        "next_cursor": next_cursor,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(response: Response) -> (StatusCode, Value) {
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        (
            parts.status,
            serde_json::from_slice(&bytes).expect("json body"),
        )
    }

    #[tokio::test]
    async fn upstream_statuses_map_to_fixed_words() {
        for (code, http, error) in [
            (tonic::Code::Unauthenticated, StatusCode::UNAUTHORIZED, "unauthorized"),
            (
                tonic::Code::PermissionDenied,
                StatusCode::FORBIDDEN,
                "space_read_not_authorized",
            ),
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST, "invalid_request"),
            (
                tonic::Code::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_artifacts_unavailable",
            ),
            (
                tonic::Code::Internal,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_artifacts_unavailable",
            ),
            (
                tonic::Code::ResourceExhausted,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_artifacts_unavailable",
            ),
            (
                tonic::Code::NotFound,
                StatusCode::SERVICE_UNAVAILABLE,
                "space_artifacts_unavailable",
            ),
        ] {
            let upstream = tonic::Status::new(code, "upstream text: decision-token-secret");
            let (status, body) = body_json(status_response(&upstream)).await;
            assert_eq!(status, http, "{code:?}");
            assert_eq!(body["error"]["code"], error, "{code:?}");
            assert_eq!(body["error"]["details"], json!({}));
            assert!(
                !body.to_string().contains("secret"),
                "{code:?} echoed the upstream text"
            );
        }
    }

    #[test]
    fn a_summary_never_carries_content_and_omits_an_empty_agent_name() {
        let mut summary = SpaceArtifactSummary {
            thread_id: "thread-1".to_owned(),
            message_id: "msg-1".to_owned(),
            artifact_id: "art-1".to_owned(),
            kind: "document".to_owned(),
            title: "Plan".to_owned(),
            version: 2,
            version_count: 3,
            size_chars: 120,
            created_at: "2027-01-15T08:00:00.000000Z".to_owned(),
            space_id: "space-room".to_owned(),
            agent_name: String::new(),
        };
        let value = summary_json(&summary);
        assert!(value.get("agent_name").is_none());
        assert!(value.get("content").is_none());
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "artifact_id",
                "created_at",
                "kind",
                "message_id",
                "size_chars",
                "space_id",
                "thread_id",
                "title",
                "version",
                "version_count"
            ]
        );
        summary.agent_name = "Researcher".to_owned();
        assert_eq!(summary_json(&summary)["agent_name"], "Researcher");
    }

    #[test]
    fn the_query_carries_the_decision_pair_and_paging_only() {
        let query: ListSpaceArtifactsQuery = serde_json::from_value(json!({
            "limit": 25,
            "cursor": "c1",
            "space_read_decision_ref": "dec-1",
            "space_read_decision_token": "tok-1"
        }))
        .expect("query");
        assert_eq!(query.limit, Some(25));
        assert_eq!(query.cursor.as_deref(), Some("c1"));
        assert_eq!(query.space_read_decision_ref.as_deref(), Some("dec-1"));
        let empty: ListSpaceArtifactsQuery =
            serde_json::from_value(json!({})).expect("empty query");
        assert_eq!(empty.limit, None);
    }
}
