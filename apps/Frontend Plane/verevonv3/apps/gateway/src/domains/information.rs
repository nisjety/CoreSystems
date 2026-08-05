use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{config::AppState, envelope::ok, upstream::proxy_json};

/// Information feeds: weather, traffic, news. These are public data aggregated
/// from external sources via information-core. No session guard — the data is
/// non-personal and the internal API key on proxy_json is the auth boundary.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/information/dashboard", get(dashboard))
        .route("/api/v1/information/weather", get(weather))
        .route("/api/v1/information/traffic", get(traffic))
        .route("/api/v1/information/news", get(news))
        .with_state(state)
}

/// Batch endpoint: fans out to weather + traffic + news concurrently and returns
/// a single `InformationDashboardPayload` that the SPA's dashboard cards consume.
async fn dashboard(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let lat = params.get("lat").map(String::as_str);
    let lon = params.get("lon").map(String::as_str);
    let altitude = params.get("altitude").map(String::as_str);
    let traffic_radius = params
        .get("trafficRadius")
        .map(String::as_str)
        .unwrap_or("40");
    let news_limit = params.get("newsLimit").map(String::as_str).unwrap_or("8");
    let news_max_age = params.get("newsMaxAge").map(String::as_str).unwrap_or("24");

    let weather_url = build_url(
        &state.information_core_url,
        "/api/v1/weather",
        &[("lat", lat), ("lon", lon), ("altitude", altitude)],
    );
    let traffic_url = build_url(
        &state.information_core_url,
        "/api/v1/traffic",
        &[("radius", Some(traffic_radius)), ("lat", lat), ("lon", lon)],
    );
    let news_url = build_url(
        &state.information_core_url,
        "/api/v1/news",
        &[("limit", Some(news_limit)), ("maxAge", Some(news_max_age))],
    );

    let (weather_res, traffic_res, news_res) = tokio::join!(
        proxy_json(&state, Method::GET, &weather_url, None, None, None, None),
        proxy_json(&state, Method::GET, &traffic_url, None, None, None, None),
        proxy_json(&state, Method::GET, &news_url, None, None, None, None),
    );

    let (weather_data, weather_error) = split_result(weather_res);
    let (traffic_data, traffic_error) = split_result(traffic_res);
    let (news_data, news_error) = split_result(news_res);

    Json(json!({
        "weather": weather_data,
        "weatherError": weather_error,
        "traffic": traffic_data,
        "trafficError": traffic_error,
        "news": news_data,
        "newsError": news_error,
    }))
}

async fn weather(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let url = build_url(
        &state.information_core_url,
        "/api/v1/weather",
        &[
            ("lat", params.get("lat").map(String::as_str)),
            ("lon", params.get("lon").map(String::as_str)),
            ("altitude", params.get("altitude").map(String::as_str)),
        ],
    );
    let (status, Json(body)) = proxy_json(&state, Method::GET, &url, None, None, None, None).await;
    (status, Json(body))
}

async fn traffic(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let url = build_url(
        &state.information_core_url,
        "/api/v1/traffic",
        &[
            ("radius", params.get("radius").map(String::as_str)),
            ("lat", params.get("lat").map(String::as_str)),
            ("lon", params.get("lon").map(String::as_str)),
            ("search", params.get("search").map(String::as_str)),
        ],
    );
    let (status, Json(body)) = proxy_json(&state, Method::GET, &url, None, None, None, None).await;
    // info-core's /traffic payload has a top-level `data` field ({ success, data: [..],
    // timestamp }), which the SPA's conditional unwrap() would strip — handing the bare
    // array to a caller typed as the full InformationTrafficPayload. Wrap on success so
    // unwrap yields the whole object (matching the dashboard endpoint, which embeds the
    // same payload). weather/news have no top-level `data`, so they correctly stay raw.
    if status.is_success() {
        (status, Json(ok(body)))
    } else {
        (status, Json(body))
    }
}

async fn news(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let url = build_url(
        &state.information_core_url,
        "/api/v1/news",
        &[
            ("limit", params.get("limit").map(String::as_str)),
            ("offset", params.get("offset").map(String::as_str)),
            ("maxAge", params.get("maxAge").map(String::as_str)),
            ("category", params.get("category").map(String::as_str)),
        ],
    );
    let (status, Json(body)) = proxy_json(&state, Method::GET, &url, None, None, None, None).await;
    (status, Json(body))
}

/// Splits a proxy result into (data_or_null, error_message_or_null).
fn split_result(result: (axum::http::StatusCode, Json<Value>)) -> (Value, Option<String>) {
    let (status, Json(body)) = result;
    if status.is_success() {
        (body, None)
    } else {
        let msg = body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::to_owned);
        (Value::Null, msg)
    }
}

fn build_url(base: &str, path: &str, params: &[(&str, Option<&str>)]) -> String {
    let qs: String = params
        .iter()
        .filter_map(|(key, val)| val.map(|v| format!("{}={}", key, urlencoding::encode(v))))
        .collect::<Vec<_>>()
        .join("&");
    if qs.is_empty() {
        format!("{}{}", base, path)
    } else {
        format!("{}{}?{}", base, path, qs)
    }
}
