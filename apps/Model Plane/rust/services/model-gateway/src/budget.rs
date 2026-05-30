//! Budget guard — pre-flight check against cost-core before forwarding to inference.
//!
//! When `max_cost_usd` or `max_tokens` are set on the request, the gateway queries
//! cost-core for current usage and rejects the request if the budget would be exceeded.

use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::normalize::NormalizedRequest;

const COST_CORE_BASE_URL_VAR: &str = "COST_CORE_URL";
const DEFAULT_COST_CORE_URL: &str = "http://localhost:8089";

#[derive(Debug, Serialize)]
struct BudgetCheckRequest {
    org_id: String,
    max_cost_usd: Option<f64>,
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BudgetCheckResponse {
    allowed: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    current_cost_usd: Option<f64>,
    #[serde(default)]
    current_tokens: Option<u64>,
}

/// Check whether the request is within budget.
///
/// Returns `Ok(())` if allowed, or an error response if budget is exceeded.
pub async fn check_budget(
    http_client: &reqwest::Client,
    org_id: &str,
    normalized: &NormalizedRequest,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if normalized.max_cost_usd.is_none() && normalized.max_tokens.is_none() {
        return Ok(());
    }

    let base_url =
        std::env::var(COST_CORE_BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_COST_CORE_URL.to_owned());
    let url = format!("{base_url}/api/v1/budget/check");

    let body = BudgetCheckRequest {
        org_id: org_id.to_owned(),
        max_cost_usd: normalized.max_cost_usd,
        max_tokens: normalized.max_tokens,
    };

    let resp = http_client
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            if let Ok(check) = r.json::<BudgetCheckResponse>().await {
                if !check.allowed {
                    let reason = check.reason.unwrap_or_else(|| "budget exceeded".to_owned());
                    tracing::warn!(
                        org_id = %org_id,
                        current_cost = ?check.current_cost_usd,
                        current_tokens = ?check.current_tokens,
                        "budget guard rejected request"
                    );
                    return Err((
                        StatusCode::PAYMENT_REQUIRED,
                        Json(json!({
                            "error": "budget_exceeded",
                            "message": reason,
                            "current_cost_usd": check.current_cost_usd,
                            "current_tokens": check.current_tokens,
                        })),
                    ));
                }
            }
            Ok(())
        }
        Ok(r) => {
            tracing::warn!(status = %r.status(), "cost-core returned non-success; allowing request");
            Ok(())
        }
        Err(e) => {
            tracing::warn!(error = %e, "cost-core unreachable; allowing request (fail-open)");
            Ok(())
        }
    }
}
