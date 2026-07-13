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
    config::AppState,
    domains::knowledge::shared,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{proxy_bearer_json, proxy_sse_stream},
};

async fn imports_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    shared::ingestion_token(state, user, &shared::cookie_header(headers)).await
}

fn imports_auth_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "imports_auth_unavailable",
            "Import authentication is temporarily unavailable.",
        )),
    )
        .into_response()
}

/// Forward multipart upload to imports-core verbatim, preserving content-type with boundary.
pub(super) async fn import_upload(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let Some(token) = imports_token(&state, &user, &headers).await else {
        return imports_auth_unavailable();
    };
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
        .bearer_auth(token)
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
) -> Response {
    let Some(token) = imports_token(&state, &user, &headers).await else {
        return imports_auth_unavailable();
    };
    let url = format!("{}/api/v1/import/jobs/source", state.imports_api_url);
    proxy_bearer_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&token),
        &user.user_id,
    )
    .await
    .into_response()
}

pub(super) async fn get_import_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = imports_token(&state, &user, &headers).await else {
        return imports_auth_unavailable();
    };
    let url = format!(
        "{}/api/v1/import/jobs/{}",
        state.imports_api_url,
        urlencoding::encode(&id)
    );
    proxy_bearer_json(&state, Method::GET, &url, None, Some(&token), &user.user_id)
        .await
        .into_response()
}

pub(super) async fn import_job_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(token) = imports_token(&state, &user, &headers).await else {
        return imports_auth_unavailable();
    };
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let url = format!(
        "{}/api/v1/import/jobs/{}/events{}",
        state.imports_api_url,
        urlencoding::encode(&id),
        shared::qs(&uri)
    );
    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        Some(&token),
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok()),
        Some((&user.user_id, &org_id)),
        false,
    )
    .await
}
