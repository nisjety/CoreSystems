//! Cost & usage read surface (Phase 7 B5).
//!
//! Proxies the Model Plane Cost Core ledger for the authenticated user's usage:
//! a filtered rollup, recent entries, and the model price catalogue. Cost Core
//! pins user-token reads to the token actor inside the session-resolved org.
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
    middleware::{has_authorized_org_role, require_session, AuthenticatedUser},
    upstream::authorized_org_id,
};

/// Default number of recent ledger entries returned by `/cost/entries`.
const DEFAULT_ENTRY_LIMIT: u32 = 50;
const MAX_ENTRY_LIMIT: u32 = 200;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/cost/summary", get(summary))
        .route("/api/v1/cost/users", get(users))
        .route("/api/v1/cost/analytics", get(analytics))
        .route("/api/v1/cost/entries", get(entries))
        .route("/api/v1/cost/azure/actual", get(azure_actual))
        .route("/api/v1/cost/pricing", get(pricing))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Azure actual charges are organization billing data; only live org owners
/// and admins can read them. The org itself is always resolved from membership.
async fn azure_actual(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !has_authorized_org_role(&user, &["owner", "admin"]) {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Organization admin access is required")),
        )
            .into_response();
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "organization_required",
                "An active organization is required",
            )),
        )
            .into_response();
    }
    let mut query = vec![format!("org_id={}", urlencoding::encode(&org_id))];
    for (input, output) in [("from", "since"), ("to", "until")] {
        if let Some(value) = params
            .get(input)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push(format!("{output}={}", urlencoding::encode(value)));
        }
    }
    let url = format!(
        "{}/api/v1/cost/azure/actual?{}",
        state.cost_core_url,
        query.join("&")
    );
    let (status, Json(body)) = proxy_cost_json(&state, Method::GET, &url, &user, &headers).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    Json(json!({ "data": normalize_azure_actual_summary(&body), "error": null })).into_response()
}

fn normalize_azure_actual_summary(body: &Value) -> Value {
    let totals: Vec<Value> =
        body.get("by_currency")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().map(|row| json!({
        "currency": row.get("currency").and_then(Value::as_str).unwrap_or_default(),
        "amount": row.get("amount").and_then(Value::as_f64).unwrap_or(0.0),
    })).collect()
            })
            .unwrap_or_default();
    json!({
        "available": body.get("available").and_then(Value::as_bool).unwrap_or(false),
        "rows": body.get("rows").and_then(Value::as_i64).unwrap_or(0),
        "byCurrency": totals,
        "latestUsageDate": body.get("latest_usage_date").cloned().unwrap_or(Value::Null),
    })
}

/// Empty rollup when there is no active organization in the session.
fn empty_summary() -> Value {
    json!({
        "totalInputTokens": 0,
        "totalOutputTokens": 0,
        "totalCostUsd": 0.0,
        "entryCount": 0,
    })
}

/// Reshape a Cost Core usage rollup. New basis fields are included only when
/// every component is present, so an older Cost Core cannot look like a verified
/// all-zero breakdown.
fn normalize_summary(body: &Value) -> Value {
    let num = |key: &str| body.get(key).and_then(Value::as_i64).unwrap_or(0);
    let mut normalized = json!({
        "totalInputTokens": num("total_input_tokens"),
        "totalOutputTokens": num("total_output_tokens"),
        "totalCostUsd": body.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        "entryCount": num("entry_count"),
    });
    let has_basis_breakdown = [
        "metered_entry_count",
        "subscription_entry_count",
        "unknown_basis_entry_count",
        "subscription_input_tokens",
        "subscription_output_tokens",
        "unknown_basis_input_tokens",
        "unknown_basis_output_tokens",
        "zero_token_entry_count",
    ]
    .iter()
    .all(|key| body.get(key).and_then(Value::as_i64).is_some())
        && body
            .get("metered_cost_usd")
            .and_then(Value::as_f64)
            .is_some();
    if has_basis_breakdown {
        normalized["basisBreakdownAvailable"] = json!(true);
        normalized["meteredEntryCount"] = json!(num("metered_entry_count"));
        normalized["subscriptionEntryCount"] = json!(num("subscription_entry_count"));
        normalized["unknownBasisEntryCount"] = json!(num("unknown_basis_entry_count"));
        normalized["meteredCostUsd"] = json!(body
            .get("metered_cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0));
        normalized["subscriptionInputTokens"] = json!(num("subscription_input_tokens"));
        normalized["subscriptionOutputTokens"] = json!(num("subscription_output_tokens"));
        normalized["unknownBasisInputTokens"] = json!(num("unknown_basis_input_tokens"));
        normalized["unknownBasisOutputTokens"] = json!(num("unknown_basis_output_tokens"));
        normalized["zeroTokenEntryCount"] = json!(num("zero_token_entry_count"));
        normalized["latestEntryAt"] = body.get("latest_entry_at").cloned().unwrap_or(Value::Null);
    }
    if let Some(start) = body.get("reporting_started_at").and_then(Value::as_str) {
        normalized["reportingStartedAt"] = json!(start);
    }
    normalized
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
        "providerUsed": str_field("provider_used"),
        "runId": str_field("run_id"),
        "requestId": str_field("request_id"),
        "inputTokens": num("input_tokens"),
        "outputTokens": num("output_tokens"),
        "costUsd": entry.get("cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        // A subscription row has real tokens and cost 0; without its basis the
        // SPA would show it as free spend. Only the two known values pass, and
        // an entry whose producer did not say is `null`.
        "costBasis": entry
            .get("cost_basis")
            .and_then(Value::as_str)
            .filter(|basis| *basis == "metered" || *basis == "subscription"),
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
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let scope = params
        .get("scope")
        .map(|value| value.trim())
        .unwrap_or("me");
    if !matches!(scope, "me" | "organization") {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_cost_scope",
                "scope must be me or organization",
            )),
        )
            .into_response();
    }
    if scope == "organization" && !has_authorized_org_role(&user, &["owner", "admin"]) {
        return (
            StatusCode::FORBIDDEN,
            Json(error("forbidden", "Organization admin access is required")),
        )
            .into_response();
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return Json(json!({ "data": empty_summary(), "error": null })).into_response();
    }

    let mut query = vec![format!("org_id={}", urlencoding::encode(&org_id))];
    if scope == "organization" {
        query.push("scope=organization".to_string());
    }
    for key in ["from", "to"] {
        if let Some(value) = params
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            let cost_core_key = if key == "from" { "since" } else { "until" };
            query.push(format!("{cost_core_key}={}", urlencoding::encode(value)));
        }
    }
    let url = format!(
        "{}/api/v1/cost/aggregate?reporting=current&{}",
        state.cost_core_url,
        query.join("&")
    );
    let (status, Json(body)) = proxy_cost_json(&state, Method::GET, &url, &user, &headers).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    let mut data = normalize_summary(&body);
    data["scope"] = json!(scope);
    Json(json!({ "data": data, "error": null })).into_response()
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

async fn users(
    State(state): State<AppState>, Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !has_authorized_org_role(&user, &["owner", "admin"]) {
        return (StatusCode::FORBIDDEN, Json(error("forbidden", "Organization admin access is required"))).into_response();
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return (StatusCode::CONFLICT, Json(error("organization_required", "Select an organization"))).into_response();
    }
    let mut query = vec![format!("org_id={}", urlencoding::encode(&org_id))];
    for (key, upstream) in [("from", "since"), ("to", "until")] {
        if let Some(value) = params.get(key).filter(|v| !v.trim().is_empty()) {
            query.push(format!("{upstream}={}", urlencoding::encode(value.trim())));
        }
    }
    let url = format!("{}/api/v1/cost/users?{}", state.cost_core_url, query.join("&"));
    proxy_cost_json(&state, Method::GET, &url, &user, &headers).await.into_response()
}

async fn analytics(
    State(state): State<AppState>, Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap, Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let scope = params.get("scope").map(String::as_str).unwrap_or("me");
    if !matches!(scope, "me" | "organization") {
        return (StatusCode::BAD_REQUEST, Json(error("invalid_cost_scope", "Invalid usage scope"))).into_response();
    }
    if scope == "organization" && !has_authorized_org_role(&user, &["owner", "admin"]) {
        return (StatusCode::FORBIDDEN, Json(error("forbidden", "Organization admin access is required"))).into_response();
    }
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return (StatusCode::CONFLICT, Json(error("organization_required", "Select an organization"))).into_response();
    }
    // Browser-supplied tenant, user and reporting-baseline selectors never pass through.
    let mut query = vec![format!("org_id={}", urlencoding::encode(&org_id)), format!("scope={scope}")];
    for (key, upstream) in [("from", "since"), ("to", "until")] {
        if let Some(value) = params.get(key).filter(|v| !v.trim().is_empty()) {
            query.push(format!("{upstream}={}", urlencoding::encode(value.trim())));
        }
    }
    let url = format!("{}/api/v1/cost/analytics?{}", state.cost_core_url, query.join("&"));
    proxy_cost_json(&state, Method::GET, &url, &user, &headers).await.into_response()
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
    fn normalize_summary_preserves_decimal_metered_breakdown() {
        let body = json!({
            "entry_count": 3,
            "metered_entry_count": 1,
            "subscription_entry_count": 1,
            "unknown_basis_entry_count": 1,
            "metered_cost_usd": 0.0123,
            "subscription_input_tokens": 20,
            "subscription_output_tokens": 5,
            "unknown_basis_input_tokens": 10,
            "unknown_basis_output_tokens": 2,
            "zero_token_entry_count": 0,
        });
        let normalized = normalize_summary(&body);
        assert_eq!(normalized["basisBreakdownAvailable"], json!(true));
        assert_eq!(normalized["meteredCostUsd"], json!(0.0123));
        assert_eq!(normalized["unknownBasisEntryCount"], json!(1));
        let mut missing = body;
        missing.as_object_mut().unwrap().remove("metered_cost_usd");
        assert!(normalize_summary(&missing)
            .get("basisBreakdownAvailable")
            .is_none());
    }

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
    fn normalize_azure_actual_summary_preserves_currency_and_source_date() {
        let normalized = normalize_azure_actual_summary(&json!({
            "available": true,
            "rows": 4,
            "by_currency": [{ "currency": "NOK", "amount": 123.45 }],
            "latest_usage_date": "2026-09-23T00:00:00Z",
        }));
        assert_eq!(normalized["available"], json!(true));
        assert_eq!(normalized["rows"], json!(4));
        assert_eq!(
            normalized["byCurrency"][0],
            json!({ "currency": "NOK", "amount": 123.45 })
        );
        assert_eq!(normalized["latestUsageDate"], json!("2026-09-23T00:00:00Z"));
    }

    #[test]
    fn normalize_entry_maps_snake_to_camel() {
        let entry = json!({
            "model": "gpt-4o-mini",
            "provider_used": "azure-norway-eu",
            "run_id": "run_123",
            "input_tokens": 100,
            "output_tokens": 50,
            "cost_usd": 0.00005,
            "created_at": "2026-06-26T10:00:00Z",
        });
        let got = normalize_entry(&entry);
        assert_eq!(got["model"], json!("gpt-4o-mini"));
        assert_eq!(got["providerUsed"], json!("azure-norway-eu"));
        assert_eq!(got["runId"], json!("run_123"));
        assert_eq!(got["inputTokens"], json!(100));
        assert_eq!(got["costUsd"], json!(0.00005));
        assert_eq!(got["createdAt"], json!("2026-06-26T10:00:00Z"));
        // No basis reported: null, never a guessed one.
        assert_eq!(got["costBasis"], Value::Null);
    }

    #[test]
    fn normalize_entry_carries_only_a_known_cost_basis() {
        let subscription = json!({
            "model": "gpt-5.6-terra",
            "input_tokens": 1200,
            "output_tokens": 300,
            "cost_usd": 0.0,
            "cost_basis": "subscription",
        });
        assert_eq!(
            normalize_entry(&subscription)["costBasis"],
            json!("subscription")
        );
        let metered = json!({ "cost_usd": 0.01, "cost_basis": "metered" });
        assert_eq!(normalize_entry(&metered)["costBasis"], json!("metered"));
        let unknown = json!({ "cost_usd": 0.0, "cost_basis": "free" });
        assert_eq!(normalize_entry(&unknown)["costBasis"], Value::Null);
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
