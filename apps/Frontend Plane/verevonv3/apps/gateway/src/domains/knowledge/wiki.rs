use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::knowledge::shared,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
};

pub(super) async fn list_wiki_pages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/wiki/pages{}", state.wiki_store_url, shared::qs(&uri));
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn wiki_page_by_path(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/by-path{}",
        state.wiki_store_url,
        shared::qs(&uri)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn get_wiki_page(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}",
        state.wiki_store_url,
        urlencoding::encode(&id)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn wiki_page_versions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/versions{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn wiki_page_diff(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/diff{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

pub(super) async fn wiki_page_backlinks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/pages/{}/backlinks{}",
        state.wiki_store_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

/// `GET /api/v1/knowledge/wiki/proposals`: the org's wiki proposals. The query passes through unchanged, so the
/// Data Plane reads `status` (pending when it is absent), `limit` and `offset`. It answers 404 while its
/// create-page proposal switch is off.
pub(super) async fn list_wiki_proposals(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/wiki/proposals{}",
        state.wiki_store_url,
        shared::qs(&uri)
    );
    shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await
}

/// `POST /api/v1/knowledge/wiki/proposals/{id}/review`: a person accepts or rejects one proposal. The body names
/// the decision and nothing else. The reviewer is the session's user, and the Data Plane checks `wiki.approve`
/// and the zero-retention posture before it writes a page. A create-page accept that finds a live page at its
/// path answers 409 `wiki_proposal_conflict`: nothing was written, and the proposal is now a conflict.
pub(super) async fn review_wiki_proposal(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let decision = body
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if decision != "accept" && decision != "reject" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_decision",
                "The decision must be accept or reject.",
            )),
        )
            .into_response();
    }
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/wiki/proposals/review", state.wiki_store_url);
    let (upstream_status, Json(reply)) = shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(json!({ "proposal_id": id, "decision": decision })),
        org_id.as_deref(),
        None,
    )
    .await;
    // The Data Plane answers a conflict with the proposal and no error envelope. Name it, so the SPA can say why.
    if upstream_status == StatusCode::CONFLICT && reply.get("proposal").is_some() {
        return (
            StatusCode::CONFLICT,
            Json(error_with_details(
                "wiki_proposal_conflict",
                "A live page already exists at this path. Nothing was written, and the proposal is now marked as a conflict.",
                json!({}),
            )),
        )
            .into_response();
    }
    (upstream_status, Json(reply)).into_response()
}
