use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

use crate::{
    config::AppState,
    contracts::RecommendPlanRequest,
    envelope::ok,
    onboarding::recommendation::{build_local_recommendation, fetch_remote_recommendation},
};

pub(crate) async fn recommend_plan(
    State(state): State<AppState>,
    Json(input): Json<RecommendPlanRequest>,
) -> impl IntoResponse {
    let local = build_local_recommendation(&input.context);
    let remote = fetch_remote_recommendation(&state, &input.context).await;
    let recommendation = remote.unwrap_or(local);
    (
        StatusCode::OK,
        Json(ok(json!({ "recommendation": recommendation }))),
    )
}
