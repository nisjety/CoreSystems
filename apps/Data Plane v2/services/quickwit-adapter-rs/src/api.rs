use std::sync::Arc;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::rebuild::{self, RebuildContext};

#[derive(Debug, Deserialize)]
struct RebuildRequest {
    org_id: Option<String>,
    #[serde(default)]
    clear: bool,
}

pub fn router(ctx: Arc<RebuildContext>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .route("/admin/rebuild", post(rebuild_index))
        .with_state(ctx)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "quickwit-adapter-rs"}))
}

async fn readyz(State(ctx): State<Arc<RebuildContext>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ready",
        "service": "quickwit-adapter-rs",
        "index": ctx.quickwit.index_id(),
    }))
}

async fn rebuild_index(
    State(ctx): State<Arc<RebuildContext>>,
    Json(req): Json<RebuildRequest>,
) -> Json<serde_json::Value> {
    let org_id = req.org_id.filter(|v| !v.trim().is_empty());
    let clear = req.clear;
    tokio::spawn(async move {
        if let Err(err) = rebuild::rebuild_all(ctx, org_id, clear).await {
            tracing::error!(error = %err, "manual Quickwit rebuild failed");
        }
    });

    Json(serde_json::json!({"accepted": true}))
}
