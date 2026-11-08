//! The chat library (F-t3-080): the person's own saved chat documents, generated artifacts and
//! uploaded text, read as one filtered view of Data Plane documents.
//!
//! `GET /api/v1/chat/library?kind=generated|uploaded|all&limit=&offset=` relays documents-api's
//! `GET /v1/documents/library`. The origins are fixed here, not by the browser: `generated` is
//! `chat-artifact`, `uploaded` is `chat-attachment`, and `all` is both, so the library cannot list
//! another kind of document. The route is dark with the artifact-promote switch (404
//! `library_disabled` while it is off), since a saved copy is what that switch enables.
//!
//! Documents-api applies the viewer's own predicate (owner, org-visible, or an explicit grant), so a
//! page holds only documents the person may open. A page carries metadata, never text; the text is
//! read by id through the knowledge document route. The answer is user content and is never cached.

use axum::{
    extract::{Extension, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser, upstream::authorized_org_id,
};

use super::documents::{CHAT_ARTIFACT_ORIGIN, CHAT_ATTACHMENT_ORIGIN};

/// The page size when the browser names none, and the largest page documents-api serves.
const DEFAULT_LIBRARY_PAGE: u32 = 50;
const MAX_LIBRARY_PAGE: u32 = 100;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LibraryQuery {
    kind: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(super) async fn list_chat_library(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<LibraryQuery>,
) -> Response {
    if !state.space_features.chat_artifact_promote {
        return no_store(
            (
                StatusCode::NOT_FOUND,
                Json(error(
                    "library_disabled",
                    "The chat library is not available.",
                )),
            )
                .into_response(),
        );
    }
    let origins = match query.kind.as_deref().unwrap_or("all") {
        "generated" => vec![CHAT_ARTIFACT_ORIGIN],
        "uploaded" => vec![CHAT_ATTACHMENT_ORIGIN],
        "all" => vec![CHAT_ARTIFACT_ORIGIN, CHAT_ATTACHMENT_ORIGIN],
        _ => {
            return no_store(
                (
                    StatusCode::BAD_REQUEST,
                    Json(error(
                        "invalid_kind",
                        "kind must be generated, uploaded or all.",
                    )),
                )
                    .into_response(),
            )
        }
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIBRARY_PAGE);
    if limit == 0 || limit > MAX_LIBRARY_PAGE {
        return no_store(
            (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_paging", "limit must be from 1 to 100.")),
            )
                .into_response(),
        );
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
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
    let url = library_url(
        &state.documents_api_url,
        &origins,
        limit,
        query.offset.unwrap_or(0),
    );
    let (status, Json(payload)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        Some(org_id.as_str()),
        None,
    )
    .await;
    no_store((status, Json(payload)).into_response())
}

/// The documents-api library URL: one `origin` parameter per kind, then the page position.
fn library_url(base: &str, origins: &[&str], limit: u32, offset: u32) -> String {
    let mut params: Vec<String> = origins
        .iter()
        .map(|origin| format!("origin={origin}"))
        .collect();
    params.push(format!("limit={limit}"));
    params.push(format!("offset={offset}"));
    format!("{base}/v1/documents/library?{}", params.join("&"))
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
