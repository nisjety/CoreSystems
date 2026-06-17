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

pub(super) async fn require_org_admin(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> Result<(), GatewayJsonResponse> {
    if has_any_role(user.auth_role.as_deref(), &["admin", "superadmin"]) {
        return Ok(());
    }

    let authorized_org_id = crate::upstream::authorized_org_id(state, user).await;
    if authorized_org_id.trim().is_empty() || authorized_org_id != org_id {
        return Err(forbidden_response());
    }

    let org_role = crate::upstream::resolve_session_context(state, user)
        .await
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if has_any_role(Some(org_role.as_str()), &["owner", "admin"]) {
        Ok(())
    } else {
        Err(forbidden_response())
    }
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
