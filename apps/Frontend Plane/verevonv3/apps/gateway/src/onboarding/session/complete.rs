use axum::{
    extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Extension, Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::{ActionActor, CompleteOnboardingRequest},
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    onboarding::session::canonical_membership_active,
    upstream::proxy_json,
    utils::empty_to_none,
};

pub(crate) async fn complete_onboarding(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<CompleteOnboardingRequest>,
) -> axum::response::Response {
    // This is a session-bound mutation. Request headers never carry the
    // authenticated actor injected by `require_session`, so deriving it with
    // `actor_from_request` can silently fall back to the local development
    // identity and mark the wrong user as onboarded. Bind the delegation actor
    // directly to the validated session instead.
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let Some(org_id) = input
        .org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_required",
                "An active organization is required before onboarding can be completed.",
            )),
        )
            .into_response();
    };
    if user.active_org_id.as_deref().map(str::trim) != Some(org_id) {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_not_ready",
                "The requested organization is not active for this session.",
            )),
        )
            .into_response();
    }
    if !canonical_membership_active(&state, &headers, org_id, &user.user_id).await {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_membership_stale",
                "Your organization access changed. Ask an owner for a new invitation before completing onboarding.",
            )),
        )
            .into_response();
    }

    let (org_status, _) = proxy_json(
        &state,
        Method::GET,
        &format!(
            "{}/orgs/{}",
            state.org_core_url,
            urlencoding::encode(org_id),
        ),
        None,
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    if !org_status.is_success() {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_recovery_required",
                "Organization membership is still being reconciled. Your progress was kept; please retry.",
            )),
        )
            .into_response();
    }

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

    if !status.is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "onboarding_completion_failed",
                "Onboarding could not be completed safely. Your progress was kept; please retry.",
            )),
        )
            .into_response();
    }

    // user-core is the canonical onboarding-completion commit. The org-core
    // status is a repairable projection: after canonical success we must never
    // return an error that invites the client to repeat an already-committed
    // completion. Reconciliation can safely replay this idempotent projection.
    let org_projection_synced = persist_org_onboarding_completion(&state, &input, &actor).await;

    crate::upstream::invalidate_session_context_cache(
        &state,
        &actor.user_id,
        input.org_id.as_deref(),
    )
    .await;

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
            "completed": true,
            "configured": true,
            "orgProjectionSynced": org_projection_synced,
            "orgId": input.org_id,
            "plan": input.plan,
            "source": input.source.unwrap_or_else(|| "verevon-v3".into()),
            "metadata": input.metadata,
        }))),
    )
        .into_response()
}

async fn persist_org_onboarding_completion(
    state: &AppState,
    input: &CompleteOnboardingRequest,
    actor: &crate::contracts::ActionActor,
) -> bool {
    let Some(org_id) = input
        .org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };

    let body = json!({
        "status": "completed",
        "steps": {
            "plan": input.plan.as_deref(),
            "source": input.source.as_deref().unwrap_or("verevon-v3"),
            "metadata": input.metadata.clone().unwrap_or(Value::Null),
        },
    });

    let url = format!(
        "{}/internal/orgs/{}/onboarding/state",
        state.org_core_url,
        urlencoding::encode(org_id),
    );
    let (status, _) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id),
        Some(actor),
        Some("application/json"),
    )
    .await;
    status.is_success()
}
