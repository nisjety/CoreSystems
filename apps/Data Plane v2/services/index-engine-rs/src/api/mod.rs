use axum::{routing::get, Json, Router};

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "index-engine-rs"}))
}

async fn readyz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ready", "service": "index-engine-rs"}))
}
