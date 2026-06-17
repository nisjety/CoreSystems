use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    auth::actor_from_headers, config::AppState, envelope::ok, upstream::proxy_json,
    utils::empty_to_none,
};

pub(crate) async fn session_bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let actor = actor_from_headers(&headers, state.allow_dev_actor_headers);
    (
        StatusCode::OK,
        Json(ok(json!({
            "actor": {
                "userId": actor.user_id,
                "userEmail": empty_to_none(&actor.user_email),
                "userName": empty_to_none(&actor.user_name),
            },
            "features": {
                "allowDevActorHeaders": state.allow_dev_actor_headers,
            },
        }))),
    )
}

/// Real onboarding status, sourced from user-core's session-context (the single
/// source of truth for completion + active org). `onboardingStatus` is one of
/// CREATED | PROFILE_READY | COMPLETED. The frontend uses `completed`/`orgId`
/// to decide post-login routing (onboarding vs dashboard).
pub(crate) async fn onboarding_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let actor = actor_from_headers(&headers, state.allow_dev_actor_headers);

    let (status, Json(ctx)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/me/session-context", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;

    let onboarding_status = ctx
        .get("onboardingStatus")
        .and_then(Value::as_str)
        .unwrap_or("CREATED");
    let completed = onboarding_status == "COMPLETED";
    let org_id = ctx.get("orgId").and_then(Value::as_str);

    // Never surface an upstream error status here — the frontend treats a 2xx
    // with completed:false as "send the user into onboarding".
    let safe_status = if status.is_success() {
        status
    } else {
        StatusCode::OK
    };

    (
        safe_status,
        Json(ok(json!({
            "configured": completed,
            "completed": completed,
            "onboardingStatus": onboarding_status,
            "orgId": org_id,
            "actor": {
                "userId": actor.user_id,
                "userEmail": empty_to_none(&actor.user_email),
            },
        }))),
    )
}
