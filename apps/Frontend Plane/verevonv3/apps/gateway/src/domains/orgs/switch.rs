use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    envelope::error,
    middleware::{invalidate_session_validation_cache, AuthenticatedUser},
    upstream::{browser_origin, proxy_auth},
};

use super::shared::cookie_header;

pub(super) async fn list_orgs(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    let url = format!("{}/api/auth/organization/list", state.auth_core_url);
    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(crate) async fn switch_active_org(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    let organization_id = body
        .as_ref()
        .and_then(|body| body.get("organizationId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| valid_organization_id(value));
    let Some(organization_id) = organization_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "Organization id must be a bounded opaque identifier.",
            )),
        )
            .into_response();
    };
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    let url = format!("{}/api/auth/organization/set-active", state.auth_core_url);
    let response = proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(serde_json::json!({ "organizationId": organization_id })),
        Some(&cookie),
        origin.as_deref(),
    )
    .await;
    if response.status().is_success() {
        invalidate_session_validation_cache(&state, &cookie).await;
        crate::upstream::invalidate_session_context_cache(
            &state,
            &user.user_id,
            user.active_org_id.as_deref(),
        )
        .await;
        crate::upstream::invalidate_session_context_cache(
            &state,
            &user.user_id,
            Some(organization_id),
        )
        .await;
    }
    response
}

fn valid_organization_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::valid_organization_id;

    #[test]
    fn organization_switch_ids_are_bounded_opaque_identifiers() {
        assert!(valid_organization_id("org_123-abc"));
        assert!(!valid_organization_id(""));
        assert!(!valid_organization_id("../org"));
        assert!(!valid_organization_id(&"o".repeat(257)));
    }
}
