use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{config::AppState, domains::chat::shared, middleware::AuthenticatedUser};

/// Forward multipart document upload to model-gateway verbatim, preserving content-type boundary.
pub(super) async fn upload_chat_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let data_plane_token = shared::data_plane_token(&state, &user, &headers).await;
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();

    let url = format!("{}/v1/chat/documents", state.model_gateway_url);
    let mut req = state
        .client
        .post(&url)
        .header("content-type", content_type)
        .header("x-user-id", &user.user_id);

    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(value) = data_plane_token
        .as_deref()
        .and_then(shared::data_plane_authorization_value)
    {
        req = req.header("x-data-plane-authorization", value);
    }

    match req.body(body).send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let resp_body = upstream.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(resp_body)).into_response()
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response(),
    }
}
