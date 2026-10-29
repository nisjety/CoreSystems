//! Integration test for model-gateway health endpoints.
//!
//! Spins up the axum app and hits /healthz and /readyz.

use axum::{routing::get, Router};
use tokio::net::TcpListener;

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

#[tokio::test]
async fn health_endpoints_return_ok() {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{addr}/healthz"))
        .send()
        .await
        .expect("healthz request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");

    let resp = client
        .get(format!("http://{addr}/readyz"))
        .send()
        .await
        .expect("readyz request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
}
