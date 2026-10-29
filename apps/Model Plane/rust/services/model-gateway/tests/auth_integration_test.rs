//! Integration tests for `require_auth` middleware.
//!
//! These tests exercise the middleware wiring via the dev-bypass path and
//! rejection paths. The RS256/JWKS crypto path is covered by the upstream
//! `jsonwebtoken` crate; exercising it here would require standing up a mock
//! JWKS endpoint and is complicated by the process-global `JWKS_CACHE`
//! `OnceCell`. See `src/auth.rs` unit test for `Claims` serialization.

use axum::{
    body::Body,
    extract::Request,
    http::{header::AUTHORIZATION, StatusCode},
    middleware,
    response::IntoResponse,
    routing::get,
    Extension, Router,
};
use model_gateway::auth::{require_auth, Claims};
use tower::ServiceExt;

fn test_router() -> Router {
    async fn handler(Extension(claims): Extension<Claims>) -> impl IntoResponse {
        (StatusCode::OK, claims.user_id)
    }
    Router::new()
        .route("/protected", get(handler))
        .layer(middleware::from_fn(require_auth))
}

#[tokio::test]
#[serial_test::serial]
async fn rejects_missing_authorization_header() {
    // Ensure dev bypass is OFF for this case.
    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");

    let app = test_router();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial_test::serial]
async fn rejects_empty_bearer_token() {
    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");

    let app = test_router();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(AUTHORIZATION, "Bearer ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial_test::serial]
async fn rejects_non_bearer_scheme() {
    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");

    let app = test_router();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(AUTHORIZATION, "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial_test::serial]
async fn dev_bypass_injects_stub_claims() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");

    let app = test_router();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(AUTHORIZATION, "Bearer any-token-value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"user_placeholder");

    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
}

#[tokio::test]
#[serial_test::serial]
async fn dev_bypass_accepts_true_case_insensitive() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "TRUE");

    let app = test_router();
    let res = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(AUTHORIZATION, "Bearer x")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
}
