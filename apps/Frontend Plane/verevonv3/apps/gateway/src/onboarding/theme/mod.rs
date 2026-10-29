mod appearance;
mod validation;

use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use reqwest::Method;
use serde_json::json;

use crate::{
    auth::actor_from_request,
    config::AppState,
    contracts::UpdateThemeRequest,
    envelope::{error, ok, unwrap_data},
    upstream::proxy_json,
};

use appearance::appearance_update_body;
use validation::is_hex_color;

pub(crate) async fn update_brand_theme(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateThemeRequest>,
) -> impl IntoResponse {
    let actor = actor_from_request(
        input.actor.as_ref(),
        Some(&headers),
        state.allow_dev_actor_headers,
    );
    let mode = input.mode.trim().to_lowercase();
    let color = input.primary_color.trim().to_lowercase();
    if mode != "verevon" && mode != "brand" {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_brand_theme",
                "Mode must be verevon or brand.",
            )),
        );
    }
    if !is_hex_color(&color) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_brand_theme",
                "Primary color must be a hex value.",
            )),
        );
    }

    let current = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/settings/appearance", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;
    let current_body = unwrap_data(&(current.1).0);
    let body = appearance_update_body(&mode, &color, &current_body);

    let (status, Json(response_body)) = proxy_json(
        &state,
        Method::PUT,
        &format!("{}/api/v1/settings/appearance", state.user_core_url),
        Some(body),
        None,
        Some(&actor),
        Some("application/json"),
    )
    .await;

    (
        StatusCode::OK,
        Json(ok(json!({
            "persisted": status.is_success(),
            "configured": status.is_success(),
            "mode": mode,
            "primaryColor": color,
            "appearance": unwrap_data(&response_body),
        }))),
    )
}
