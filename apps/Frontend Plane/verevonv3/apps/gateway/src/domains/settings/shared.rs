use axum::http::HeaderMap;

use crate::{contracts::ActionActor, middleware::AuthenticatedUser};

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(super) fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}
