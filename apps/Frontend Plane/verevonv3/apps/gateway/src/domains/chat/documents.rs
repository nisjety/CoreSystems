use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{config::AppState, middleware::AuthenticatedUser};

/// Ephemeral extraction: imports-core authenticates the ingestion audience and
/// reads bytes in memory. No document/job is created and no storage scope is accepted.
pub(crate) async fn extract_chat_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let Some(token) = super::shared::ingestion_token(&state, &user, &headers).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(crate::envelope::error(
            "imports_auth_unavailable", "Document reading is temporarily unavailable.",
        ))).into_response();
    };
    let mut response = crate::upstream::proxy_bearer_json(
        &state, Method::POST, &format!("{}/api/v1/import/extract", state.imports_api_url),
        Some(payload), Some(&token), &user.user_id,
    ).await.into_response();
    response.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    response
}

/// Non-empty trimmed string field, or `None`.
fn text_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// POST /api/v1/chat/documents — persist a chat text attachment as a durable
/// Data Plane v2 document so retrieval can ground later turns on it.
///
/// This used to forward to model-gateway `/v1/chat/documents`, which is dead:
/// that handler calls `DocumentService.CreateDocument` over gRPC, and Data
/// Plane v2 has disabled the method, answering every call with
/// "gRPC DocumentService.CreateDocument is disabled; use documents-api-go
/// POST /v1/documents instead". The whole chain returned 502, so the upload was
/// unreachable rather than merely unwired. We now take the route Data Plane v2
/// names, reusing the same documents-api leg the knowledge domain already
/// proxies through, which mints the scoped `data-plane` audience token.
pub(crate) async fn upload_chat_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_body",
                    "message": "Expected a JSON object with `title` and `content`."
                }
            })),
        )
            .into_response();
    };

    // Title and content are the document; without them there is nothing to
    // ground on, and documents-api would reject the row anyway.
    let (Some(title), Some(content)) = (
        text_field(&payload, "title"),
        text_field(&payload, "content"),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_body",
                    "message": "`title` and `content` are both required and must be non-empty."
                }
            })),
        )
            .into_response();
    };

    let org_id = {
        let resolved = crate::upstream::authorized_org_id(&state, &user).await;
        (!resolved.is_empty()).then_some(resolved)
    };

    let document = json!({
        "org_id": org_id.clone().unwrap_or_default(),
        "source": text_field(&payload, "source").unwrap_or_else(|| "chat-upload".to_owned()),
        "type": text_field(&payload, "type").unwrap_or_else(|| "text".to_owned()),
        "title": title,
        "content": content,
        "zdr_classification": "internal",
        "metadata": { "origin": "chat-attachment" },
    });

    // documents-api answers with the document row itself, whose `document_id`
    // and `status` are the two fields the composer reports back to the user, so
    // the upstream body passes through unchanged.
    let url = format!("{}/v1/documents", state.documents_api_url);
    crate::domains::knowledge::shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(document),
        org_id.as_deref(),
        None,
    )
    .await
    .into_response()
}
