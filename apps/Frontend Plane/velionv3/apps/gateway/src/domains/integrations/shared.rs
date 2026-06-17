use axum::http::HeaderMap;

use crate::{contracts::ActionActor, middleware::AuthenticatedUser};

pub(super) fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

pub(super) fn org_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-velion-org-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
}
