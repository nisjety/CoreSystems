//! WS10.S1 router-level tests for the `x-request-id` layer.
//!
//! Drives the real router (`routes::router`). Auth uses the dev bypass
//! (`QUARRY_EDGE_AUTH_DEV_BYPASS=1`, read once per process): a bearer that is
//! not a verifiable JWT becomes the stub claims. The envelope route is
//! `POST /v1/scrape` with a URL that does not parse, which answers 400 with the
//! canonical `{ data, meta, error }` envelope.

mod support;

use std::sync::{Arc, Once};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use quarry_edge::routes::router;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::Value;
use tokio::sync::mpsc;
use tower::ServiceExt;

fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1"));
}

fn app() -> Router {
    init();
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    router(support::base_state(driver, EventSink::new(tx)))
}

async fn scrape(inbound: Option<&str>, authed: bool) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/scrape")
        .header("content-type", "application/json");
    if authed {
        builder = builder.header("authorization", "Bearer not-a-jwt");
    }
    if let Some(id) = inbound {
        builder = builder.header("x-request-id", id);
    }
    app()
        .oneshot(
            builder
                .body(Body::from(r#"{"url":"not a url"}"#))
                .expect("request"),
        )
        .await
        .expect("router answers")
}

#[tokio::test]
async fn meta_request_id_equals_the_response_header() {
    let response = scrape(Some("e2e-meta-check-1"), true).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let header = response
        .headers()
        .get("x-request-id")
        .expect("header echoed")
        .to_str()
        .expect("ascii")
        .to_owned();
    assert_eq!(header, "e2e-meta-check-1");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("json envelope");
    assert_eq!(body["meta"]["request_id"], header.as_str());
}

#[tokio::test]
async fn the_request_id_is_not_an_auth_or_tenancy_key() {
    // The same id that a signed-in caller sends does not authenticate an
    // anonymous one: the protected route still answers 401.
    let response = scrape(Some("e2e-foreign-tenant-id"), false).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let header = response
        .headers()
        .get("x-request-id")
        .expect("header echoed on the 401 too")
        .to_str()
        .expect("ascii");
    assert_eq!(header, "e2e-foreign-tenant-id");
}

#[tokio::test]
async fn a_bad_inbound_id_is_replaced_not_rejected() {
    let overlong = "x".repeat(300);
    let response = scrape(Some(&overlong), true).await;
    // The 400 is the bad URL, not the id: the request is still answered.
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let header = response
        .headers()
        .get("x-request-id")
        .expect("header echoed")
        .to_str()
        .expect("ascii")
        .to_owned();
    assert!(header.starts_with("req_"), "{header}");
    assert_ne!(header, overlong);
}
