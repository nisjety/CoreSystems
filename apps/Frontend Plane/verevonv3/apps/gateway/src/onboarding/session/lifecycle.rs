use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::browser_origin,
    upstream::proxy_json,
};

pub(crate) async fn onboarding_lifecycle(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let Some(org_id) = user
        .active_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_not_ready",
                "Your organization is not active yet. Return to the organization step and retry.",
            )),
        )
            .into_response();
    };

    if !canonical_membership_active(&state, &headers, org_id, &user.user_id).await {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_membership_stale",
                "Your organization access changed. Ask an owner for a new invitation before continuing.",
            )),
        )
            .into_response();
    }

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let (status, _) = proxy_json(
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
    if !status.is_success() {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "organization_recovery_required",
                "Your organization membership is still being reconciled. Retry before choosing a plan.",
            )),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(ok(json!({
            "state": "PROFILE_READY",
            "orgId": org_id,
            "retryable": false,
        }))),
    )
        .into_response()
}

pub(crate) async fn canonical_membership_active(
    state: &AppState,
    headers: &HeaderMap,
    org_id: &str,
    user_id: &str,
) -> bool {
    canonical_membership_role(state, headers, org_id, user_id)
        .await
        .is_some()
}

pub(crate) async fn canonical_membership_role(
    state: &AppState,
    headers: &HeaderMap,
    org_id: &str,
    user_id: &str,
) -> Option<String> {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let url = format!(
        "{}/api/auth/organization/get-full-organization?organizationId={}",
        state.auth_core_url,
        urlencoding::encode(org_id),
    );
    let mut request = state.client.get(url).header("cookie", cookie);
    if let Some(origin) = browser_origin(headers) {
        request = request.header("origin", origin);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let payload: serde_json::Value = response.json().await.ok()?;
    let organization = payload.get("data").unwrap_or(&payload);
    organization
        .get("members")?
        .as_array()?
        .iter()
        .find(|member| {
            member
                .get("userId")
                .or_else(|| member.get("user_id"))
                .and_then(serde_json::Value::as_str)
                == Some(user_id)
        })
        .and_then(|member| member.get("role"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}
