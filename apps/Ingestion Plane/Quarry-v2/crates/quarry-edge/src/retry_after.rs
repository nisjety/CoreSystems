//! `Retry-After` on the edge's own refusals.
//!
//! Handlers return a typed error whose `details.retry_after_seconds` says when
//! to come back (the run caps, the drain refusal). The handler return type
//! carries no headers, so the number lived only in the JSON body and a client
//! or proxy that honours the standard header never saw it. This layer copies
//! the number into the header on 429 and 503 responses that have a small JSON
//! body and no `Retry-After` of their own.

use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::header::{CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

/// Refusals are a few hundred bytes. Anything bigger is not one.
const MAX_REFUSAL_BODY: usize = 16 * 1024;

pub async fn retry_after_header(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let refusal = matches!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    );
    let json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if !refusal || !json || response.headers().contains_key(RETRY_AFTER) {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_REFUSAL_BODY).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "refusal body unreadable; Retry-After not added");
            return Response::from_parts(parts, Body::empty());
        }
    };
    if let Some(seconds) = retry_after_seconds(&bytes) {
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            parts.headers.insert(RETRY_AFTER, value);
        }
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// The wait the body asks for: `error.details.retry_after_seconds` in the
/// canonical envelope, or `retry_after_seconds` at the top of an `ApiError`.
fn retry_after_seconds(body: &[u8]) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value["error"]["details"]["retry_after_seconds"]
        .as_u64()
        .or_else(|| value["retry_after_seconds"].as_u64())
        .filter(|seconds| *seconds > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use axum::middleware;
    use axum::routing::get;
    use axum::{Json, Router};
    use serde_json::json;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route(
                "/capacity",
                get(|| async {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({
                            "data": null,
                            "error": { "code": "RATE_LIMITED", "details": { "retry_after_seconds": 10 } }
                        })),
                    )
                }),
            )
            .route(
                "/flat",
                get(|| async {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({ "retry_after_seconds": 60 })),
                    )
                }),
            )
            .route(
                "/own",
                get(|| async {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [(RETRY_AFTER, "3")],
                        Json(json!({ "retry_after_seconds": 60 })),
                    )
                }),
            )
            .route(
                "/no-number",
                get(|| async { (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": {} }))) }),
            )
            .route(
                "/ok",
                get(|| async { Json(json!({ "retry_after_seconds": 5 })) }),
            )
            .layer(middleware::from_fn(retry_after_header))
    }

    async fn call(path: &str) -> Response {
        app()
            .oneshot(HttpRequest::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    fn header(response: &Response) -> Option<&str> {
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
    }

    #[tokio::test]
    async fn a_capacity_refusal_gets_the_header_from_the_envelope() {
        let response = call("/capacity").await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(header(&response), Some("10"));
        // The body still reads back whole.
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["details"]["retry_after_seconds"], 10);
    }

    #[tokio::test]
    async fn the_flat_api_error_shape_is_read_too() {
        assert_eq!(header(&call("/flat").await), Some("60"));
    }

    #[tokio::test]
    async fn a_header_the_handler_set_is_not_overwritten() {
        assert_eq!(header(&call("/own").await), Some("3"));
    }

    #[tokio::test]
    async fn no_number_means_no_header() {
        assert_eq!(header(&call("/no-number").await), None);
    }

    #[tokio::test]
    async fn successful_responses_are_untouched() {
        assert_eq!(header(&call("/ok").await), None);
    }
}
