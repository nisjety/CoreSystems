use axum::{
    routing::{get, post},
    Json, Router,
};

mod embed;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .route("/v1/embed-text", post(embed::embed_text))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "embedding-engine-rs"}))
}

async fn readyz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ready", "service": "embedding-engine-rs"}))
}
