use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_request, config::AppState, contracts::CompleteOnboardingRequest, envelope::ok,
    upstream::proxy_json, utils::empty_to_none,
};

pub(crate) async fn complete_onboarding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CompleteOnboardingRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let user_identifier = empty_to_none(&actor.user_email).unwrap_or_else(|| actor.user_id.clone());

    let mut url = format!(
        "{}/api/v1/users/onboarding/complete?email={}",
        state.user_core_url,
        urlencoding::encode(&user_identifier)
    );
    if user_identifier == actor.user_id && actor.user_email.is_empty() {
        url = format!(
            "{}/api/v1/users/onboarding/complete?email={}",
            state.user_core_url,
            urlencoding::encode(&actor.user_id)
        );
    }

    let (status, _) = proxy_json(&state, Method::POST, &url, None, None, Some(&actor), None).await;

    if status.is_success() {
        persist_org_onboarding_completion(&state, &input).await;
    }

    let clear_state = json!({ "step": "", "state": Value::Null });
    let _ = proxy_json(
        &state,
        Method::PUT,
        &format!("{}/api/v1/users/me/onboarding-state", state.user_core_url),
        Some(clear_state),
        None,
        Some(&actor),
        Some("application/json"),
    )
    .await;

    (
        StatusCode::OK,
        Json(ok(json!({
            "completed": status.is_success(),
            "configured": status.is_success(),
            "orgId": input.org_id,
            "plan": input.plan,
            "source": input.source.unwrap_or_else(|| "velion-v3".into()),
            "metadata": input.metadata,
        }))),
    )
}

async fn persist_org_onboarding_completion(state: &AppState, input: &CompleteOnboardingRequest) {
    let Some(org_id) = input
        .org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    let body = json!({
        "status": "completed",
        "steps": {
            "plan": input.plan.as_deref(),
            "source": input.source.as_deref().unwrap_or("velion-v3"),
            "metadata": input.metadata.clone().unwrap_or(Value::Null),
        },
    });

    let _ = state
        .client
        .post(format!(
            "{}/internal/orgs/{}/onboarding/state",
            state.org_core_url,
            urlencoding::encode(org_id),
        ))
        .header("x-internal-api-key", &state.internal_api_key)
        .json(&body)
        .send()
        .await;
}
