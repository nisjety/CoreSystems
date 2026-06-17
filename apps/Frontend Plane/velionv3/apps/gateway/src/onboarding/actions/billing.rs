use axum::{extract::State, http::HeaderMap, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::json;

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::{ConfirmCheckoutRequest, SetPlanRequest, StartCheckoutRequest},
    upstream::proxy_json,
};

pub(crate) async fn set_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SetPlanRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "plan": input.plan,
        "reason": input.reason.unwrap_or_else(|| "onboarding".into()),
        "onboarding": input.onboarding,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/orgs/{}/plan",
            state.org_core_url,
            urlencoding::encode(input.org_id.trim())
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        None,
    )
    .await
}

pub(crate) async fn start_checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StartCheckoutRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "plan": input.plan,
        "success_url": input.success_url,
        "cancel_url": input.cancel_url,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/billing/orgs/{}/checkout-session",
            state.billing_core_url,
            urlencoding::encode(input.org_id.trim())
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        None,
    )
    .await
}

pub(crate) async fn confirm_checkout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ConfirmCheckoutRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let body = json!({
        "plan": input.plan,
        "payment_id": input.payment_id,
        "client_secret": input.client_secret,
    });

    proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/api/v1/billing/orgs/{}/checkout-session/confirm",
            state.billing_core_url,
            urlencoding::encode(input.org_id.trim())
        ),
        Some(body),
        Some(input.org_id.trim()),
        Some(&actor),
        None,
    )
    .await
}
