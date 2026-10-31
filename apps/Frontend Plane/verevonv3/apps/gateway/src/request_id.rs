//! `x-request-id` intake, echo and forwarding for the gateway (WS10.S1).
//!
//! A valid inbound `x-request-id` (1 to 128 bytes, `[A-Za-z0-9._:-]`) is
//! adopted; anything else is replaced by a minted `req_<32 hex>`. A bad id is
//! never rejected. The id is echoed on every response, recorded on the HTTP
//! trace span, and forwarded to upstream Quarry and core calls by [`apply`].
//!
//! The id is a support-lookup key. It is not unique across tenants and is never
//! an authorization, tenancy or idempotency key. Quarry applies the same rule
//! (`quarry-edge/src/request_id.rs`).

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// The wire header (lowercase).
pub const HEADER: &str = "x-request-id";
/// Largest inbound id adopted, in bytes.
pub const MAX_INBOUND_LEN: usize = 128;

tokio::task_local! {
    static REQUEST_ID: String;
}

/// The id of one request, kept in the request extensions so the trace span
/// can record it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

/// Whether an inbound id may be adopted: 1 to 128 bytes from `[A-Za-z0-9._:-]`.
pub fn is_valid_inbound(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INBOUND_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// A fresh server-minted id: `req_` followed by 32 lowercase hex digits.
pub fn mint() -> String {
    format!("req_{:032x}", rand::random::<u128>())
}

/// The id for a request: the inbound id when valid, otherwise a minted id.
pub fn resolve(inbound: Option<&str>) -> String {
    match inbound {
        Some(value) if is_valid_inbound(value) => value.to_owned(),
        _ => mint(),
    }
}

/// Test seam: runs `fut` inside a request scope with `id`, as the layer would.
#[cfg(test)]
pub(crate) async fn scope_for_test<F: std::future::Future>(id: &str, fut: F) -> F::Output {
    REQUEST_ID.scope(id.to_owned(), fut).await
}

/// The current request's id, or `None` outside a request scope.
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

/// Middleware: resolves the id, exposes it through the task-local and the
/// request extensions, and echoes it on the response.
pub async fn layer(mut request: Request, next: Next) -> Response {
    let inbound = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok());
    let id = resolve(inbound);
    request.extensions_mut().insert(RequestId(id.clone()));
    let mut response = REQUEST_ID.scope(id.clone(), next.run(request)).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(HEADER), value);
    }
    response
}

/// Adds the current request id to an outbound request, beside `x-user-id`.
/// Outside a request scope (a spawned task, a background job) nothing is added.
pub fn apply(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match current() {
        Some(id) => builder.header(HEADER, id),
        None => builder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use axum::{middleware, Router};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn adopts_a_valid_inbound_id() {
        assert_eq!(
            resolve(Some("e2e-d10-0a1b.2c:3d-4e_5f")),
            "e2e-d10-0a1b.2c:3d-4e_5f"
        );
        let edge = "c".repeat(128);
        assert_eq!(resolve(Some(edge.as_str())), edge);
    }

    #[test]
    fn replaces_overlong_and_foreign_charset_ids() {
        let overlong = "d".repeat(129);
        for bad in [
            "",
            overlong.as_str(),
            "has space",
            "tab\tthere",
            "slash/path",
            "semi;colon",
            "caf\u{e9}",
        ] {
            let id = resolve(Some(bad));
            assert!(id.starts_with("req_"), "{bad:?} -> {id}");
            assert!(is_valid_inbound(&id));
        }
        assert!(resolve(None).starts_with("req_"));
    }

    #[test]
    fn minted_ids_are_well_formed_and_distinct() {
        let a = mint();
        let b = mint();
        assert_ne!(a, b);
        assert_eq!(a.len(), "req_".len() + 32);
        assert!(a[4..].bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(is_valid_inbound(&a));
    }

    #[tokio::test]
    async fn echoes_the_id_and_exposes_it_to_the_handler() {
        async fn seen() -> String {
            current().unwrap_or_default()
        }
        let app = Router::new()
            .route("/seen", get(seen))
            .layer(middleware::from_fn(layer));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/seen")
                    .header(HEADER, "gw-trace-42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers().get(HEADER).unwrap(), "gw-trace-42");
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"gw-trace-42");
    }

    #[tokio::test]
    async fn apply_adds_the_header_only_inside_a_request_scope() {
        let client = reqwest::Client::new();
        let outside = apply(client.get("http://127.0.0.1:9/")).build().unwrap();
        assert!(outside.headers().get(HEADER).is_none());

        let inside = REQUEST_ID
            .scope("gw-scoped-1".to_owned(), async {
                apply(client.get("http://127.0.0.1:9/")).build().unwrap()
            })
            .await;
        assert_eq!(inside.headers().get(HEADER).unwrap(), "gw-scoped-1");
    }

    #[tokio::test]
    async fn invalid_inbound_request_id_is_replaced_not_forwarded() {
        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/probe"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&upstream)
            .await;
        let target = format!("{}/probe", upstream.uri());
        let app = Router::new()
            .route(
                "/proxy",
                get(move || {
                    let target = target.clone();
                    async move {
                        let status = apply(reqwest::Client::new().get(target))
                            .send()
                            .await
                            .map(|r| r.status().as_u16())
                            .unwrap_or(0);
                        status.to_string()
                    }
                }),
            )
            .layer(middleware::from_fn(layer));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/proxy")
                    .header(HEADER, "bad id with spaces")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let echoed = response
            .headers()
            .get(HEADER)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(echoed.starts_with("req_"), "{echoed}");

        let received = upstream.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let forwarded = received[0].headers.get(HEADER).unwrap().to_str().unwrap();
        assert_eq!(forwarded, echoed);
    }
}
