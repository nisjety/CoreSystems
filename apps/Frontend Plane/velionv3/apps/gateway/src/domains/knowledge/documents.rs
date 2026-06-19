use axum::{
    extract::{Extension, Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::knowledge::shared, envelope::error, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

pub(super) async fn list_documents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/documents{}",
        state.documents_api_url,
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

/// Ingest a curated document straight into Data Plane v2 (`documents-api`
/// `POST /v1/documents`). Used by the scrape-preview "add selected sections"
/// flow: the SPA sends `{ title, content, sourceUrl?, type? }` (the user-chosen
/// markdown subset) and the gateway fills the rest of the Data Plane contract
/// (source, zdr_classification, org scoping) server-side.
pub(super) async fn create_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let content = body.get("content").and_then(Value::as_str).unwrap_or("");
    if content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_content", "content is required")),
        )
            .into_response();
    }

    let source_url = body
        .get("sourceUrl")
        .or_else(|| body.get("source_url"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(if source_url.is_empty() {
            None
        } else {
            Some(source_url)
        })
        .unwrap_or("Untitled");
    let doc_type = body
        .get("type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("web");

    let payload = json!({
        "org_id": org_id.clone().unwrap_or_default(),
        "source": "velion-scrape",
        "type": doc_type,
        "title": title,
        "content": content,
        "zdr_classification": "internal",
        "metadata": { "source_url": source_url },
    });

    let url = format!("{}/v1/documents", state.documents_api_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(payload),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
    .into_response()
}

pub(super) async fn get_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!(
        "{}/v1/documents/{}",
        state.documents_api_url,
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

pub(super) async fn list_sources(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> impl IntoResponse {
    let org_id = {
        let o = crate::upstream::authorized_org_id(&state, &user).await;
        (!o.is_empty()).then_some(o)
    };
    let url = format!("{}/v1/sources{}", state.documents_api_url, shared::qs(&uri));
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
