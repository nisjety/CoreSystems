//! Cost & usage read surface (Phase 7 B5).
//!
//! Proxies the Model Plane cost ledger (cost-core) so the SPA can render real
//! per-org spend: a rolled-up summary, the most recent cost-bearing entries, and
//! the model price catalogue (rate card). The org is resolved server-side from
//! the validated session and passed as the `org_id` filter — a client can never
//! read another org's ledger (IDOR-clean).
//!
//! HONESTY: every figure is whatever cost-core actually recorded. cost-core
//! prices each inference from its catalogue (migration 0002), so an org with no
//! activity yields a zeroed summary and an empty entry list — never a fabricated
//! spend. The gateway only reshapes snake_case → camelCase for the SPA.

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{
    audience_tokens::{require_model_service_token, ModelServiceAudience},
    config::AppState,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::authorized_org_id,
};

/// Default number of recent ledger entries returned by `/cost/entries`.
const DEFAULT_ENTRY_LIMIT: u32 = 50;
const MAX_ENTRY_LIMIT: u32 = 200;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/cost/summary", get(summary))
        .route("/api/v1/cost/entries", get(entries))
        .route("/api/v1/cost/pricing", get(pricing))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// The honest empty summary: no recorded cost for this org.
fn empty_summary() -> Value {
    json!({
        "totalInputTokens": 0,
        "totalOutputTokens": 0,
        "totalCostUsd": 0.0,
        "entryCount": 0,
    })
}

/// Reshape a cost-core usage rollup (`total_*` snake_case) to the SPA's camelCase
/// summary. Missing fields collapse to zero — never a fabricated total.
fn normalize_summary(body: &Value) -> Value {
    let num = |key: &str| body.get(key).and_then(Value::as_i64).unwrap_or(0);
    json!({
        "totalInputTokens": num("total_input_tokens"),
        "totalOutputTokens": num("total_output_tokens"),
        "totalCostUsd": body.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        "entryCount": num("entry_count"),
    })
}

/// Reshape one cost-core ledger entry to the SPA's camelCase row.
fn normalize_entry(entry: &Value) -> Value {
    let str_field = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let num = |key: &str| entry.get(key).and_then(Value::as_i64).unwrap_or(0);
    json!({
        "model": str_field("model"),
        "runId": str_field("run_id"),
        "requestId": str_field("request_id"),
        "inputTokens": num("input_tokens"),
        "outputTokens": num("output_tokens"),
        "costUsd": entry.get("cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        "createdAt": str_field("created_at"),
    })
}

/// Reshape one cost-core pricing row to the SPA's camelCase rate-card entry.
fn normalize_rate(rate: &Value) -> Value {
    let str_field = |key: &str| {
        rate.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let num = |key: &str| rate.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let currency = {
        let c = str_field("currency");
        if c.is_empty() {
            "USD".to_string()
        } else {
            c
        }
    };
    json!({
        "model": str_field("model"),
        "inputPerMillion": num("input_per_million"),
        "outputPerMillion": num("output_per_million"),
        "currency": currency,
    })
}

async fn summary(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return Json(json!({ "data": empty_summary(), "error": null })).into_response();
    }

    let url = format!(
        "{}/api/v1/cost/aggregate?org_id={}",
        state.cost_core_url,
        urlencoding::encode(&org_id)
    );
    let (status, Json(body)) = proxy_cost_json(&state, Method::GET, &url, &user, &headers).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    Json(json!({ "data": normalize_summary(&body), "error": null })).into_response()
}

async fn entries(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return Json(json!({ "data": [], "meta": { "count": 0 }, "error": null })).into_response();
    }

    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_ENTRY_LIMIT)
        .clamp(1, MAX_ENTRY_LIMIT);

    let url = format!(
        "{}/api/v1/cost/entries?org_id={}&limit={}",
        state.cost_core_url,
        urlencoding::encode(&org_id),
        limit
    );
    let (status, Json(body)) = proxy_cost_json(&state, Method::GET, &url, &user, &headers).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }

    let rows: Vec<Value> = body
        .get("entries")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(normalize_entry).collect())
        .unwrap_or_default();
    let count = rows.len();
    Json(json!({ "data": rows, "meta": { "count": count }, "error": null })).into_response()
}

async fn pricing(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // The catalogue is global (not org-scoped); still session-gated by the
    // route_layer above.
    let url = format!("{}/api/v1/pricing", state.cost_core_url);
    let (status, Json(body)) = proxy_cost_json(&state, Method::GET, &url, &user, &headers).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    let rates: Vec<Value> = body
        .get("rates")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(normalize_rate).collect())
        .unwrap_or_default();
    let count = rates.len();
    Json(json!({ "data": rates, "meta": { "count": count }, "error": null })).into_response()
}

async fn proxy_cost_json(
    state: &AppState,
    method: Method,
    url: &str,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> (StatusCode, Json<Value>) {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let Ok(token) =
        require_model_service_token(state, &user.user_id, cookie, ModelServiceAudience::CostCore)
            .await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "cost_auth_unavailable",
                "Cost service authentication is unavailable",
            )),
        );
    };
    let response = match state
        .client
        .request(method, url)
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            );
        }
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    (status, Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_summary_maps_totals_and_collapses_missing_to_zero() {
        let body = json!({
            "total_input_tokens": 1200,
            "total_output_tokens": 340,
            "total_cost_usd": 0.0123,
            "entry_count": 5,
        });
        assert_eq!(
            normalize_summary(&body),
            json!({
                "totalInputTokens": 1200,
                "totalOutputTokens": 340,
                "totalCostUsd": 0.0123,
                "entryCount": 5,
            })
        );
        // Missing fields → an honest zeroed summary, never fabricated.
        assert_eq!(normalize_summary(&json!({})), empty_summary());
    }

    #[test]
    fn normalize_entry_maps_snake_to_camel() {
        let entry = json!({
            "model": "gpt-4o-mini",
            "run_id": "run_123",
            "input_tokens": 100,
            "output_tokens": 50,
            "cost_usd": 0.00005,
            "created_at": "2026-06-26T10:00:00Z",
        });
        let got = normalize_entry(&entry);
        assert_eq!(got["model"], json!("gpt-4o-mini"));
        assert_eq!(got["runId"], json!("run_123"));
        assert_eq!(got["inputTokens"], json!(100));
        assert_eq!(got["costUsd"], json!(0.00005));
        assert_eq!(got["createdAt"], json!("2026-06-26T10:00:00Z"));
    }

    #[test]
    fn normalize_rate_defaults_currency_to_usd() {
        let rate =
            json!({ "model": "default", "input_per_million": 3.0, "output_per_million": 15.0 });
        let got = normalize_rate(&rate);
        assert_eq!(got["inputPerMillion"], json!(3.0));
        assert_eq!(got["currency"], json!("USD"));
    }
}
