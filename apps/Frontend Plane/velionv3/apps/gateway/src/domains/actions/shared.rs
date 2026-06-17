use axum::http::HeaderMap;

use crate::{audience_tokens::get_audience_token, config::AppState, middleware::AuthenticatedUser};

pub(super) fn org_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-velion-org-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
}

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(super) async fn quarry_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}
