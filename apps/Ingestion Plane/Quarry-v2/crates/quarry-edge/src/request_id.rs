//! `x-request-id` intake and echo for quarry-edge (WS10.S1).
//!
//! Every request gets one id. A valid inbound `x-request-id` (1 to 128 bytes,
//! `[A-Za-z0-9._:-]`) is adopted; anything else, including an overlong or
//! malformed value, is replaced by a server-minted `req_<ULID>`. A bad id is
//! never a 400. The id is echoed in the `x-request-id` response header and is
//! the `meta.request_id` of every envelope (handlers read it with [`current`]).
//!
//! The id is a support-lookup key. It is not unique across tenants, so it is
//! never an authorization, tenancy or idempotency key: support lookups filter
//! by `(org_id, request_id)`.
//!
//! Flag: `QUARRY_EDGE__REQUEST_ID_ADOPT`. Unset or any value other than
//! `0`/`false`/`off` adopts valid inbound ids (the default). `0` mints every
//! id, which is the safety valve.

use std::sync::OnceLock;

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use quarry_core::ids::kinds::RequestKind;
use quarry_runtime::request_context;
use tracing::Span;

/// The wire header name (lowercase; HTTP header names are case-insensitive).
pub const HEADER: &str = "x-request-id";
/// Operator switch for adopting inbound ids. `0` mints every id.
pub const ADOPT_ENV: &str = "QUARRY_EDGE__REQUEST_ID_ADOPT";
/// Largest inbound id adopted, in bytes.
pub const MAX_INBOUND_LEN: usize = 128;

/// The id of one request, kept in the request extensions so the trace span
/// (see [`make_request_span`]) can record it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

/// Whether an inbound id is safe to adopt: 1 to 128 bytes from `[A-Za-z0-9._:-]`.
pub fn is_valid_inbound(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INBOUND_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// A fresh server-minted id, `req_<ULID>`.
pub fn mint() -> String {
    RequestKind::new().to_string()
}

/// The id for a request: the inbound id when adoption is on and it is valid,
/// otherwise a minted id. Never fails.
pub fn resolve(inbound: Option<&str>, adopt: bool) -> String {
    match inbound {
        Some(value) if adopt && is_valid_inbound(value) => value.to_owned(),
        _ => mint(),
    }
}

/// Reads the adoption switch from its environment value. Pure, so the flag
/// is testable without touching the process environment.
pub fn adopt_from_env(value: Option<&str>) -> bool {
    !matches!(value.map(str::trim), Some("0" | "false" | "off"))
}

fn adopt_enabled() -> bool {
    static ADOPT: OnceLock<bool> = OnceLock::new();
    *ADOPT.get_or_init(|| adopt_from_env(std::env::var(ADOPT_ENV).ok().as_deref()))
}

/// The current request's id for handlers. Inside a request it is the id the
/// layer resolved, so `meta.request_id` equals the response header. Outside a
/// request (a spawned task, a direct call in a test) it mints a fresh id.
pub fn current() -> String {
    request_context::current_opt().unwrap_or_else(mint)
}

/// Middleware: resolves the id, exposes it to the handler through the
/// request-context task-local, records it in the request extensions for the
/// trace span, and echoes it on the response (success and error alike).
pub async fn layer(mut request: Request, next: Next) -> Response {
    let inbound = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok());
    let id = resolve(inbound, adopt_enabled());
    request.extensions_mut().insert(RequestId(id.clone()));
    let mut response = request_context::scope(id.clone(), next.run(request)).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(HEADER), value);
    }
    response
}

/// The request span: method, path (never the query string) and the request
/// id. Replaces `zdr_log::make_request_span` in the router's `TraceLayer`; the
/// `zdr` field is still declared empty so `zdr_log::mark_floor` can record it.
pub fn make_request_span<B>(request: &axum::http::Request<B>) -> Span {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.0.as_str())
        .unwrap_or("");
    tracing::info_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
        request_id = %request_id,
        zdr = tracing::field::Empty,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::{middleware, Router};
    use tower::ServiceExt;

    #[test]
    fn adopts_a_valid_inbound_id() {
        let inbound = "e2e-d10-0a1b.2c3d:4e-5f_6";
        assert_eq!(resolve(Some(inbound), true), inbound);
        assert!(is_valid_inbound(inbound));
    }

    #[test]
    fn mints_when_the_inbound_id_is_longer_than_128_bytes() {
        let long = "a".repeat(129);
        let id = resolve(Some(&long), true);
        assert!(id.starts_with("req_"), "{id}");
        assert!(is_valid_inbound(&id));
        // Exactly 128 bytes is still adopted.
        let edge = "b".repeat(128);
        assert_eq!(resolve(Some(&edge), true), edge);
    }

    #[test]
    fn mints_when_the_inbound_id_has_spaces_or_control_characters() {
        for bad in [
            "has space",
            "tab\there",
            "line\nbreak",
            "semi;colon",
            "slash/path",
            "uni\u{e9}",
        ] {
            let id = resolve(Some(bad), true);
            assert!(id.starts_with("req_"), "{bad:?} -> {id}");
        }
        assert!(!is_valid_inbound(""));
    }

    #[test]
    fn never_returns_400_for_a_bad_id() {
        // `resolve` has no error path: whatever arrives, the result is a
        // valid id. The HTTP layer answers the request normally (checked by
        // the router test in `tests/request_id.rs`).
        let hostile = [
            "",
            " ",
            "\u{0}",
            "../../etc/passwd",
            "\u{1F600}",
            &"x".repeat(10_000),
        ];
        for value in hostile {
            assert!(is_valid_inbound(&resolve(Some(value), true)));
        }
        assert!(is_valid_inbound(&resolve(None, true)));
    }

    #[test]
    fn adopt_flag_off_always_mints() {
        let id = resolve(Some("valid-id"), false);
        assert!(id.starts_with("req_"), "{id}");
        assert!(!adopt_from_env(Some("0")));
        assert!(!adopt_from_env(Some(" false ")));
        assert!(!adopt_from_env(Some("off")));
        assert!(adopt_from_env(Some("1")));
        assert!(adopt_from_env(None));
    }

    #[tokio::test]
    async fn echoes_the_header_on_success_and_error_responses() {
        async fn ok() -> &'static str {
            "fine"
        }
        async fn handler_sees_the_id() -> String {
            request_context::current_opt().unwrap_or_default()
        }
        async fn fails() -> StatusCode {
            StatusCode::BAD_REQUEST
        }
        let app = Router::new()
            .route("/ok", get(ok))
            .route("/seen", get(handler_sees_the_id))
            .route("/bad", get(fails))
            .layer(middleware::from_fn(layer));

        let ok_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/ok")
                    .header(HEADER, "trace-abc-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_resp.headers().get(HEADER).unwrap(), "trace-abc-123");

        let seen_resp = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/seen")
                    .header(HEADER, "trace-abc-456")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(seen_resp.headers().get(HEADER).unwrap(), "trace-abc-456");
        let body = axum::body::to_bytes(seen_resp.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"trace-abc-456");

        let bad_resp = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/bad")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);
        let minted = bad_resp.headers().get(HEADER).unwrap().to_str().unwrap();
        assert!(minted.starts_with("req_"), "{minted}");
    }
}
