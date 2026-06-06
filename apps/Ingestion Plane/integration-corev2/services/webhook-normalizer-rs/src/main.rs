use anyhow::Context;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use integration_webhook_normalizer::{normalize, NormalizeRequest};
use serde::Serialize;
use std::{env, net::SocketAddr};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    service_name: &'static str,
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let port = env::var("PORT").unwrap_or_else(|_| "3036".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .with_context(|| format!("parse PORT={port}"))?;
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(health))
        .route("/v1/webhooks/normalize", post(normalize_webhook))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState {
            service_name: "integration-webhook-normalizer",
        });

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "starting integration webhook normalizer");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve integration webhook normalizer")?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": state.service_name,
    }))
}

async fn normalize_webhook(Json(request): Json<NormalizeRequest>) -> impl IntoResponse {
    match normalize(request) {
        Ok(normalized) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                data: Some(normalized),
                error: None,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<serde_json::Value> {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        )
            .into_response(),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
