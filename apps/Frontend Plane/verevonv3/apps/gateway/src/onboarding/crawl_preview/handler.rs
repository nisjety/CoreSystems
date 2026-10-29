use crate::{
    audience_tokens::get_onboarding_preview_token,
    config::AppState,
    contracts::CrawlPreviewRequest,
    middleware::AuthenticatedUser,
    onboarding::crawl_preview::{quarry::capture_preview, sse::sse_json},
    public_url::normalize_public_http_url,
};
use async_stream::stream;
use axum::{
    extract::State,
    http::HeaderMap,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Extension, Json,
};
use serde_json::json;

pub(crate) async fn crawl_preview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<CrawlPreviewRequest>,
) -> impl IntoResponse {
    let url = normalize_public_http_url(&input.url);
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let token = get_onboarding_preview_token(&state, &user.user_id, cookie).await;
    // Pre-workspace preview is bounded public evidence, not a durable crawl
    // or knowledge-base receipt. Quarry validates/pins every redirect target.
    let output = stream! {
        let receipt = match (url, token) {
            (Ok(url), Some(token)) => {
                yield Ok::<Event, std::convert::Infallible>(sse_json("started", json!({"url":url,"target":1})));
                capture_preview(&state, &token, &url).await
            }
            _ => Err(anyhow::anyhow!("preview unavailable")),
        };
        match receipt {
            Ok(receipt) => {
                let elements = receipt.elements;
                yield Ok(sse_json("snippet", receipt.snippet));
                yield Ok(sse_json("done", json!({"count":1,"pages":1,"elements":elements,"status":"completed"})));
            }
            Err(_) => {
                yield Ok(sse_json("warning", json!({"code":"preview_unavailable","message":"Kunne ikke lese en trygg forhåndsvisning av nettsiden. Prøv igjen, eller fortsett uten forhåndsvisning."})));
                yield Ok(sse_json("done", json!({"count":0,"pages":0,"elements":0,"status":"failed"})));
            }
        }
    };
    Sse::new(output)
        .keep_alive(KeepAlive::default())
        .into_response()
}
