use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::{actor_from_headers, actor_from_request},
    config::AppState,
    contracts::{OnboardingStateResponse, OnboardingStateWriteRequest},
    envelope::{ok, unwrap_data},
    upstream::proxy_json,
};

pub(crate) async fn get_onboarding_state(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let actor = actor_from_headers(&headers, state.allow_dev_actor_headers);

    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/users/me/onboarding-state", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;

    let data = unwrap_data(&body);
    let step = data
        .get("step")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let state_payload = data.get("state").cloned();

    let safe_status = if status.is_success() {
        status
    } else {
        StatusCode::OK
    };
    (
        safe_status,
        Json(ok(OnboardingStateResponse {
            step,
            state: state_payload,
        })),
    )
}

pub(crate) async fn put_onboarding_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<OnboardingStateWriteRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "step": input.step.trim(),
        "state": input.state,
    });

    let (status, _) = proxy_json(
        &state,
        Method::PUT,
        &format!("{}/api/v1/users/me/onboarding-state", state.user_core_url),
        Some(body),
        None,
        Some(&actor),
        Some("application/json"),
    )
    .await;

    (
        StatusCode::OK,
        Json(ok(json!({
            "success": status.is_success(),
            "configured": status.is_success(),
        }))),
    )
}
