use axum::{extract::State, http::HeaderMap, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::json;

use crate::{
    auth::actor_from_request, config::AppState, contracts::WebsiteIngestRequest, envelope::error,
    public_url::normalize_public_http_url, upstream::proxy_json, utils::trim_opt,
};

pub(crate) async fn start_website_ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WebsiteIngestRequest>,
) -> impl IntoResponse {
    let url = match normalize_public_http_url(&input.url) {
        Ok(url) => url,
        Err(message) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(error("invalid_url", message)),
            );
        }
    };

    let request_body = json!({
        "kind": "crawl",
        "params": {
            "url": url,
            "max_pages": input.max_pages.unwrap_or(8).clamp(1, 20),
            "max_depth": 1,
            "auto_commit": true,
            "org_id": input.org_id.trim(),
            "brief": trim_opt(input.brief),
        }
    });
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );

    proxy_json(
        &state,
        Method::POST,
        &format!("{}/v1/jobs/", state.quarry_control_url),
        Some(request_body),
        None,
        Some(&actor),
        Some("application/json"),
    )
    .await
}
