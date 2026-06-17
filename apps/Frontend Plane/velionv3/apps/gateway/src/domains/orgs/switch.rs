use axum::{extract::State, http::HeaderMap, response::Response, Json};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, upstream::proxy_auth};

use super::shared::cookie_header;

pub(super) async fn switch_active_org(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    let cookie = cookie_header(&headers);
    let url = format!("{}/api/auth/organization/set-active", state.auth_core_url);
    proxy_auth(&state, Method::POST, &url, body.map(|b| b.0), Some(&cookie)).await
}
