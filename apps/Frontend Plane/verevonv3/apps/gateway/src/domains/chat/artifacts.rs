//! Artifact revision history and the composer's capability read (Wave 3 EP-3 S3).
//!
//! Three read-only pass-throughs to Model Gateway:
//! - `GET /api/v1/chat/threads/{thread_id}/artifacts`: the head of each artifact, no text.
//! - `GET /api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}/revisions`: one artifact's
//!   revisions newest first, paged with `before_version` and `limit`.
//! - `GET /api/v1/chat/composer-capabilities`: which composer and panel features the gateway
//!   serves, from its own switches, so the SPA needs no build-time flag.
//!
//! The gateway and Session Core decide who may read a thread: the BFF forwards the user's minted
//! model and session tokens only, never an owner, org or Space header of its own, and relays the
//! gateway's typed envelope and status unchanged (404 `artifact_history_unavailable` while the
//! history switch is not `on`). Revision text is the user's content, so no answer is cached.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser, upstream::authorized_org_id,
};

use super::shared;

/// The gateway's page cap; a larger ask is clamped there too.
const MAX_REVISION_PAGE: u32 = 20;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RevisionPageQuery {
    #[serde(alias = "beforeVersion")]
    before_version: Option<u32>,
    limit: Option<u32>,
}

pub(super) async fn list_artifact_heads(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> Response {
    let url = heads_url(&state.model_gateway_url, &thread_id);
    relay(&state, &user, &headers, &url).await
}

pub(super) async fn list_artifact_revisions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((thread_id, artifact_id)): Path<(String, String)>,
    Query(query): Query<RevisionPageQuery>,
) -> Response {
    let url = revisions_url(&state.model_gateway_url, &thread_id, &artifact_id, &query);
    relay(&state, &user, &headers, &url).await
}

pub(super) async fn get_composer_capabilities(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let url = format!("{}/v1/chat/composer-capabilities", state.model_gateway_url);
    let response = relay(&state, &user, &headers, &url).await;
    super::documents::with_promote_capability(response, state.space_features.chat_artifact_promote).await
}

/// One GET to Model Gateway under the user's own tokens, relayed with `no-store`.
async fn relay(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    url: &str,
) -> Response {
    if authorized_org_id(state, user).await.trim().is_empty() {
        return no_store(
            (
                StatusCode::FORBIDDEN,
                Json(error(
                    "org_scope_required",
                    "An authorized organization scope is required.",
                )),
            )
                .into_response(),
        );
    }
    let token = shared::model_token(state, user, headers).await;
    let session_token = match shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(error) => return no_store(shared::delegated_auth_unavailable(error).into_response()),
    };
    let (status, Json(payload)): (StatusCode, Json<Value>) =
        shared::proxy_model_json_with_session(
            state,
            Method::GET,
            url,
            None,
            token.as_deref(),
            Some(&session_token),
            user,
        )
        .await;
    no_store((status, Json(payload)).into_response())
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn heads_url(base: &str, thread_id: &str) -> String {
    format!("{base}/v1/threads/{}/artifacts", urlencoding::encode(thread_id))
}

/// Only the two paging numbers are relayed; anything else in the browser's query is dropped.
fn revisions_url(base: &str, thread_id: &str, artifact_id: &str, query: &RevisionPageQuery) -> String {
    let mut params = Vec::new();
    if let Some(before) = query.before_version.filter(|version| *version > 0) {
        params.push(format!("before_version={before}"));
    }
    if let Some(limit) = query.limit.filter(|limit| *limit > 0) {
        params.push(format!("limit={}", limit.min(MAX_REVISION_PAGE)));
    }
    let mut url = format!(
        "{base}/v1/threads/{}/artifacts/{}/revisions",
        urlencoding::encode(thread_id),
        urlencoding::encode(artifact_id),
    );
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segments_are_encoded_so_an_id_cannot_reach_another_route() {
        assert_eq!(
            heads_url("http://mg", "t/../runs"),
            "http://mg/v1/threads/t%2F..%2Fruns/artifacts"
        );
        let url = revisions_url("http://mg", "t1", "a b/c", &RevisionPageQuery::default());
        assert_eq!(url, "http://mg/v1/threads/t1/artifacts/a%20b%2Fc/revisions");
    }

    #[test]
    fn only_positive_paging_numbers_are_relayed_and_the_page_is_capped() {
        let url = revisions_url(
            "http://mg",
            "t1",
            "a1",
            &RevisionPageQuery {
                before_version: Some(5),
                limit: Some(500),
            },
        );
        assert_eq!(
            url,
            "http://mg/v1/threads/t1/artifacts/a1/revisions?before_version=5&limit=20"
        );
        let url = revisions_url(
            "http://mg",
            "t1",
            "a1",
            &RevisionPageQuery {
                before_version: Some(0),
                limit: Some(0),
            },
        );
        assert!(!url.contains('?'));
    }

    #[test]
    fn the_paging_query_accepts_the_spa_spelling() {
        let query: RevisionPageQuery =
            serde_json::from_value(serde_json::json!({ "beforeVersion": 3, "limit": 2 })).unwrap();
        assert_eq!(query.before_version, Some(3));
        assert_eq!(query.limit, Some(2));
    }

    #[test]
    fn every_answer_is_marked_no_store() {
        let response = no_store(
            (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": {} }))).into_response(),
        );
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
}
