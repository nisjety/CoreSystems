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

async fn readyz() -> impl axum::response::IntoResponse {
    // The GDPR erasure consumer is part of readiness because its failure was
    // otherwise invisible: the supervisor retries forever and only logs, so this
    // process reported ready while org erasure silently stopped being applied.
    let erasure = nats_connection::erasure_health::readiness();
    let status = if erasure.is_ready() {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({
            "status": if erasure.is_ready() { "ready" } else { "not_ready" },
            "service": "embedding-engine-rs",
            "checks": { "gdpr_erasure_consumer": erasure.as_str() }
        })),
    )
}
