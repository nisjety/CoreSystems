use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use reqwest::Method;

use crate::{config::AppState, contracts::BrregSearchQuery, envelope::error, upstream::proxy_json};

pub(crate) async fn brreg_search(
    State(state): State<AppState>,
    Query(query): Query<BrregSearchQuery>,
) -> impl IntoResponse {
    let q = query.q.trim();
    if q.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_query", "Query is required.")),
        );
    }

    let upstream = format!(
        "{}/api/v1/brreg/search?q={}&size={}",
        state.org_core_url,
        urlencoding::encode(q),
        query.size.unwrap_or(8).clamp(1, 20)
    );

    proxy_json(&state, Method::GET, &upstream, None, None, None, None).await
}
