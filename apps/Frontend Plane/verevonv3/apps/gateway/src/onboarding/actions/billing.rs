use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::{ConfirmCheckoutRequest, SetPlanRequest, StartCheckoutRequest},
    domains::billing::canonical_checkout_body,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    onboarding::session::canonical_membership_role,
    upstream::proxy_json,
};

pub(crate) async fn set_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<SetPlanRequest>,
) -> Response {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    if let Err(response) =
        checkout_lifecycle_ready(&state, &user, &actor, &headers, &input.org_id).await
    {
        return response;
    }
    // A paywall selection is not an entitlement grant. The selected plan is
    // acknowledged for UI continuity; Billing Core may activate it only after
    // provider-confirmed checkout.
    (
        StatusCode::OK,
        Json(ok(json!({
            "orgId": input.org_id.trim(),
            "selectedPlan": input.plan,
            "reason": input.reason.unwrap_or_else(|| "onboarding".into()),
            "onboarding": input.onboarding,
            "activated": false,
        }))),
    )
        .into_response()
}

pub(crate) async fn start_checkout(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<StartCheckoutRequest>,
) -> Response {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    if let Err(response) =
        checkout_lifecycle_ready(&state, &user, &actor, &headers, &input.org_id).await
    {
        return response;
    }
    let body = match canonical_checkout_body(
        &json!({ "plan": input.plan }),
        &state.verevon_public_origin,
        "/onboarding",
    ) {
        Ok(body) => body,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_checkout_request", message)),
            )
                .into_response()
        }
    };

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
    .into_response()
}

pub(crate) async fn confirm_checkout(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<ConfirmCheckoutRequest>,
) -> Response {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    if let Err(response) =
        checkout_lifecycle_ready(&state, &user, &actor, &headers, &input.org_id).await
    {
        return response;
    }
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
    .into_response()
}

async fn checkout_lifecycle_ready(
    state: &AppState,
    user: &AuthenticatedUser,
    actor: &crate::contracts::ActionActor,
    headers: &HeaderMap,
    requested_org_id: &str,
) -> Result<(), Response> {
    let requested_org_id = requested_org_id.trim();
    if requested_org_id.is_empty()
        || user.active_org_id.as_deref().map(str::trim) != Some(requested_org_id)
    {
        return Err((
            StatusCode::CONFLICT,
            Json(error(
                "organization_not_ready",
                "An active, provisioned organization is required before checkout.",
            )),
        )
            .into_response());
    }

    let Some(role) =
        canonical_membership_role(state, headers, requested_org_id, &user.user_id).await
    else {
        return Err((
            StatusCode::CONFLICT,
            Json(error(
                "organization_membership_stale",
                "Your organization access changed. Ask an owner for a new invitation before checkout.",
            )),
        )
            .into_response());
    };
    if !role
        .split(',')
        .any(|value| matches!(value.trim(), "owner" | "admin"))
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "billing_admin_required",
                "Only an organization owner or administrator can choose a plan or manage checkout.",
            )),
        )
            .into_response());
    }

    let (status, _) = proxy_json(
        state,
        Method::GET,
        &format!(
            "{}/orgs/{}",
            state.org_core_url,
            urlencoding::encode(requested_org_id),
        ),
        None,
        Some(requested_org_id),
        Some(actor),
        None,
    )
    .await;
    if !status.is_success() {
        return Err((
            StatusCode::CONFLICT,
            Json(error(
                "organization_recovery_required",
                "Organization membership is still being reconciled. Retry before continuing to checkout.",
            )),
        )
            .into_response());
    }

    Ok(())
}
