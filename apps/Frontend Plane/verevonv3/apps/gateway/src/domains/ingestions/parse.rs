//! Document reading for the Innhenting page, under `/api/ingestions/parse` (Quarry INNH-11).
//!
//! The body is the document's raw bytes, a PDF or a DOCX: not multipart and not a URL. It is
//! forwarded as it arrived, with the caller's own `Content-Type`, and Quarry decides the kind
//! from the bytes. Quarry stores nothing and redacts nothing, so personal data in a document
//! comes back as it is, to the signed-in caller only; this handler keeps nothing and logs no
//! body. The answer is `no-store`, and `Retry-After` is passed on so the page can wait.
//! Quarry answers 501 while its `QUARRY_EDGE__PARSE_ROUTE` switch is off, and that passes
//! through unchanged.

use std::time::Duration;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Extension, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, upstream_unavailable},
    middleware::AuthenticatedUser,
};

use super::shared::{cookie_header, no_store, quarry_token};

/// The largest document Quarry reads: the parsers' own limit, 25 MiB.
const MAX_DOCUMENT_BYTES: usize = 25 * 1024 * 1024;

/// Quarry gives up after 15 s of parsing. The gateway allows for the upload as well, so a slow
/// link is not reported as a gateway timeout.
const PARSE_TIMEOUT: Duration = Duration::from_secs(60);

/// `POST /api/ingestions/parse` — a PDF or DOCX read to Markdown, with nothing stored.
pub(super) async fn parse_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let bytes = match read_document(&headers, body).await {
        Ok(bytes) => bytes,
        Err(refusal) => return refusal,
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let url = format!("{}/v1/parse", state.quarry_edge_url);
    let mut request = crate::request_id::apply(state.client.post(&url))
        .timeout(PARSE_TIMEOUT)
        .header("x-user-id", user.user_id.as_str());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    // The type the caller declared goes on as it is: Quarry judges it against the bytes.
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        request = request.header(header::CONTENT_TYPE, content_type.clone());
    }
    // A retry is made only when no connection was made, so the document is not sent twice.
    let upstream = match crate::upstream::send_with_retry(request.body(bytes)).await {
        Ok(upstream) => upstream,
        Err(err) if err.is_timeout() => {
            return no_store((
                StatusCode::GATEWAY_TIMEOUT,
                Json(error(
                    "upstream_timeout",
                    "The document took too long to read.",
                )),
            ))
        }
        Err(_) => {
            return no_store((StatusCode::BAD_GATEWAY, Json(upstream_unavailable())));
        }
    };
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let retry_after = upstream.headers().get(header::RETRY_AFTER).cloned();
    let Ok(reply) = upstream.json::<Value>().await else {
        return no_store((StatusCode::BAD_GATEWAY, Json(upstream_unavailable())));
    };
    let mut response = no_store((status, Json(reply)));
    if let Some(value) = retry_after {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// The document's bytes, or the 413 answer. A declared length over the limit is refused before
/// anything is read. Without one, the read stops at the limit. A read that fails for any other
/// reason is refused the same way: past the limit is the failure this route expects, and the
/// caller is not sent a document the gateway could not read whole.
async fn read_document(headers: &HeaderMap, body: Body) -> Result<Bytes, Response> {
    if declared_length(headers).is_some_and(|length| length > MAX_DOCUMENT_BYTES as u64) {
        return Err(too_large());
    }
    to_bytes(body, MAX_DOCUMENT_BYTES)
        .await
        .map_err(|_| too_large())
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The standard envelope, with the limit named in `details` as Quarry names it.
fn too_large() -> Response {
    let mut body = error("too_large", "The document is larger than 25 MiB.");
    body["error"]["details"] = json!({ "max_bytes": MAX_DOCUMENT_BYTES });
    no_store((StatusCode::PAYLOAD_TOO_LARGE, Json(body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body_of(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn the_limit_is_the_parsers_own_25_mib() {
        assert_eq!(MAX_DOCUMENT_BYTES, 25 * 1024 * 1024);
    }

    #[test]
    fn a_declared_length_is_read_from_the_header_only() {
        let mut headers = HeaderMap::new();
        assert_eq!(declared_length(&headers), None);
        headers.insert(header::CONTENT_LENGTH, "1024".parse().unwrap());
        assert_eq!(declared_length(&headers), Some(1024));
        headers.insert(header::CONTENT_LENGTH, "lots".parse().unwrap());
        assert_eq!(declared_length(&headers), None);
    }

    #[tokio::test]
    async fn a_declared_length_over_the_limit_is_refused_before_the_body_is_read() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_LENGTH,
            (MAX_DOCUMENT_BYTES + 1).to_string().parse().unwrap(),
        );
        // An empty body: the refusal comes from the header alone.
        let refusal = read_document(&headers, Body::empty()).await.unwrap_err();
        assert_eq!(refusal.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            refusal.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let body = body_of(refusal).await;
        assert_eq!(body["error"]["code"], "too_large");
        assert_eq!(body["error"]["details"]["max_bytes"], MAX_DOCUMENT_BYTES);
    }

    #[tokio::test]
    async fn a_body_past_the_limit_without_a_declared_length_is_refused() {
        let body = Body::from(vec![0_u8; MAX_DOCUMENT_BYTES + 1]);
        let refusal = read_document(&HeaderMap::new(), body).await.unwrap_err();
        assert_eq!(refusal.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn a_body_at_the_limit_is_read_whole() {
        let body = Body::from(vec![7_u8; MAX_DOCUMENT_BYTES]);
        let Ok(bytes) = read_document(&HeaderMap::new(), body).await else {
            panic!("a body at the limit is read whole");
        };
        assert_eq!(bytes.len(), MAX_DOCUMENT_BYTES);
    }
}
