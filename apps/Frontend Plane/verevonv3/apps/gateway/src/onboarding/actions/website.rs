use axum::{extract::State, http::HeaderMap, response::IntoResponse, Extension, Json};
use reqwest::Method;
use serde_json::json;

use crate::{
    audience_tokens::get_audience_token, config::AppState, contracts::WebsiteIngestRequest,
    domains::web_domain_policy::guard_crawl, envelope::error, middleware::AuthenticatedUser,
    public_url::normalize_public_http_url, upstream::proxy_bearer_json,
};

pub(crate) async fn start_website_ingest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
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

    // Crawl handoff through quarry-edge — the only sanctioned cross-plane
    // Ingestion entrypoint. Org/tenant identity comes from the verified JWT
    // claims, so `org_id` no longer travels in the body (and the old
    // `auto_commit`/`brief` params were dead — the orchestrator never read
    // them). `ingest: true` is the real durable-persist knob: the onboarding
    // website becomes user-owned knowledge in the Data Plane.
    let mut request_body = json!({
        "url": url,
        "max_pages": input.max_pages.unwrap_or(8).clamp(1, 20),
        "max_depth": 1,
        "ingest": true,
    });

    // P2-bff-01 rel1 (EP-9 O-23): the website is a crawl. The org's web domain rules judge its seed host and ride on the
    // crawl for Quarry to apply to every link it follows. A refused seed, or rules that cannot be read, ends the start
    // with a typed refusal before anything is sent. Off (`WEB_DOMAIN_POLICY_BFF`, the default), the body is what it was.
    if let Some(refusal) = guard_crawl(&state, &user, &headers, &mut request_body).await {
        return refusal;
    }

    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let token = get_audience_token(&state, &user.user_id, &cookie, "quarry").await;

    let (status, body) = proxy_bearer_json(
        &state,
        Method::POST,
        &format!("{}/v1/crawl", state.quarry_edge_url),
        Some(request_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    (status, body)
}
