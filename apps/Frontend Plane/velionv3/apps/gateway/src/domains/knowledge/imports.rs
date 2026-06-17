use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::knowledge::shared, envelope::error, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

/// Forward multipart upload to imports-core verbatim, preserving content-type with boundary.
pub(super) async fn import_upload(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let org_id = shared::org_id_from_headers(&headers).unwrap_or_default();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();

    let url = format!("{}/api/v1/import/jobs/upload", state.imports_api_url);
    let req = state
        .client
        .post(&url)
        .header("content-type", content_type)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-org-id", &org_id)
        .header("x-user-id", &user.user_id)
        .body(body);

    match req.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let resp_body = upstream.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(resp_body)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", e.to_string())),
        )
            .into_response(),
    }
}

pub(super) async fn import_source(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/api/v1/import/jobs/source", state.imports_api_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn get_import_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!(
        "{}/api/v1/import/jobs/{}",
        state.imports_api_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn import_job_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!(
        "{}/api/v1/import/jobs/{}/events{}",
        state.imports_api_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}
