use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

use crate::{
    config::AppState, contracts::ActionActor, envelope::error, middleware::AuthenticatedUser,
};

pub(super) type GatewayJsonResponse = (StatusCode, Json<Value>);

pub(super) fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

pub(super) fn require_active_org(
    user: &AuthenticatedUser,
    requested_org_id: &str,
) -> Result<(), GatewayJsonResponse> {
    let Some(membership) = user.authorized_membership.as_ref() else {
        return Err(forbidden_org_response());
    };
    if membership.organization_id != requested_org_id.trim() {
        return Err(forbidden_org_response());
    }
    Ok(())
}

pub(super) async fn require_org_admin(
    _state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<(), GatewayJsonResponse> {
    require_active_org(user, org_id)?;
    if user
        .authorized_membership
        .as_ref()
        .is_some_and(|membership| has_any_role(Some(&membership.role), &["owner", "admin"]))
    {
        Ok(())
    } else {
        Err(forbidden_response())
    }
}

fn forbidden_org_response() -> GatewayJsonResponse {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "forbidden",
            "The requested organization is not authorized for this session.",
        )),
    )
}

fn forbidden_response() -> GatewayJsonResponse {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "forbidden",
            "Organization administration requires an owner or admin role.",
        )),
    )
}

fn has_any_role(value: Option<&str>, allowed: &[&str]) -> bool {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|role| {
            allowed
                .iter()
                .any(|allowed_role| role.eq_ignore_ascii_case(allowed_role))
        })
}

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}
