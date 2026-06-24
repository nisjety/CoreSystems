use axum::{
    extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Extension, Json,
};
use serde_json::json;

use crate::{
    config::AppState,
    contracts::RecommendPlanRequest,
    envelope::ok,
    middleware::AuthenticatedUser,
    onboarding::recommendation::{build_local_recommendation, fetch_remote_recommendation},
};

pub(crate) async fn recommend_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<RecommendPlanRequest>,
) -> impl IntoResponse {
    // The onboarding router is behind `require_session`, so the caller's
    // identity + cookie are available here — pass them to the remote call so it
    // can mint a model-plane token and reach the AI recommender (the previous
    // signature had no auth, so the model call 401'd and always fell back to
    // the local heuristic).
    let local = build_local_recommendation(&input.context);
    let remote = fetch_remote_recommendation(&state, &user, &headers, &input.context).await;
    let recommendation = remote.unwrap_or(local);
    (
        StatusCode::OK,
        Json(ok(json!({ "recommendation": recommendation }))),
    )
}
