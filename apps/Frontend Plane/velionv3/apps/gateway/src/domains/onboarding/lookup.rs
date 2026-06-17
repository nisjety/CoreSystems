use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    config::AppState,
    onboarding::{
        crawl_preview::crawl_preview,
        lookup::{brreg_search, graph_preview, recommend_plan},
    },
};

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/onboarding/brreg/search", get(brreg_search))
        .route("/api/v1/onboarding/graph-preview", get(graph_preview))
        .route("/api/v1/onboarding/crawl-preview", post(crawl_preview))
        .route("/api/v1/onboarding/recommend-plan", post(recommend_plan))
}
