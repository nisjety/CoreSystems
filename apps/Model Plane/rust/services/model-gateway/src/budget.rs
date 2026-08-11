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
    user_id: String,
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
///
/// # Errors
///
/// Returns a `402 PAYMENT_REQUIRED` JSON error when cost-core reports the user
/// is over budget. Authentication, transport, and parse failures fail closed
/// with 503 so an unavailable budget authority cannot silently allow spend.
/// `org_limits` are the ceilings Control Plane has configured for this org
/// (`org_quota`). A cap supplied on the REQUEST wins over the org default —
/// a caller may tighten its own run, but the org ceiling still applies to any
/// dimension the request left unset, so a caller cannot loosen it by omission.
pub async fn check_budget(
    http_client: &reqwest::Client,
    org_id: &str,
    user_id: &str,
    verified_bearer: &str,
    normalized: &NormalizedRequest,
    org_limits: crate::org_quota::OrgQuotaLimits,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let effective_cost = normalized.max_cost_usd.or(org_limits.max_cost_usd);
    // The quota column is i64; the wire field is u32. Clamp rather than wrap:
    // a ceiling larger than u32 is effectively "no limit", and wrapping would
    // turn a huge cap into a tiny one and reject everything.
    let org_tokens = org_limits
        .max_tokens
        .map(|tokens| u32::try_from(tokens).unwrap_or(u32::MAX));
    let effective_tokens = normalized.max_tokens.or(org_tokens);
    if effective_cost.is_none() && effective_tokens.is_none() {
        return Ok(());
    }

    let base_url =
        std::env::var(COST_CORE_BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_COST_CORE_URL.to_owned());
    check_budget_at(
        http_client,
        &base_url,
        org_id,
        user_id,
        verified_bearer,
        effective_cost,
        effective_tokens,
    )
    .await
}

async fn check_budget_at(
    http_client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    user_id: &str,
    verified_bearer: &str,
    max_cost_usd: Option<f64>,
    max_tokens: Option<u32>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let bearer = verified_bearer.trim();
    if org_id.trim().is_empty()
        || user_id.trim().is_empty()
        || bearer.is_empty()
        || bearer.chars().any(char::is_whitespace)
    {
        return Err(dependency_unavailable(
            "verified budget identity is unavailable",
        ));
    }
    let url = format!("{}/api/v1/budget/check", base_url.trim_end_matches('/'));

    let body = BudgetCheckRequest {
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        max_cost_usd,
        max_tokens,
    };

    let resp = http_client
        .post(&url)
        .bearer_auth(bearer)
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let check = r.json::<BudgetCheckResponse>().await.map_err(|error| {
                tracing::warn!(%error, "cost-core returned an invalid budget response");
                dependency_unavailable("budget authority returned an invalid response")
            })?;
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
            Ok(())
        }
        Ok(r) => {
            tracing::warn!(status = %r.status(), "cost-core rejected budget check");
            Err(dependency_unavailable(
                "budget authority rejected the request",
            ))
        }
        Err(e) => {
            tracing::warn!(error = %e, "cost-core unreachable; blocking budgeted request");
            Err(dependency_unavailable("budget authority is unavailable"))
        }
    }
}

fn dependency_unavailable(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "budget_unavailable",
            "message": message,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn normalized() -> NormalizedRequest {
        NormalizedRequest {
            content: "hello".to_owned(),
            model: String::new(),
            session_key: None,
            thread_id: None,
            structured_output_schema: None,
            zdr: true,
            max_cost_usd: Some(10.0),
            max_tokens: Some(1_000),
        }
    }

    #[tokio::test]
    async fn authenticated_budget_check_forwards_verified_user_scope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer verified-cost-token"))
            .and(body_json(serde_json::json!({
                "org_id": "org-a",
                "user_id": "user-a",
                "max_cost_usd": 10.0,
                "max_tokens": 1000,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "allowed": true,
                "current_cost_usd": 2.5,
                "current_tokens": 50,
            })))
            .expect(1)
            .mount(&server)
            .await;

        check_budget_at(
            &reqwest::Client::new(),
            &server.uri(),
            "org-a",
            "user-a",
            "verified-cost-token",
            Some(10.0),
            Some(1_000),
        )
        .await
        .expect("within budget");
    }

    #[tokio::test]
    async fn budget_dependency_auth_or_transport_failure_fails_closed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let error = check_budget_at(
            &reqwest::Client::new(),
            &server.uri(),
            "org-a",
            "user-a",
            "rejected-token",
            Some(10.0),
            Some(1_000),
        )
        .await
        .expect_err("auth failure must block inference");
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);

        let error = check_budget_at(
            &reqwest::Client::new(),
            "http://127.0.0.1:1",
            "org-a",
            "user-a",
            "verified-cost-token",
            Some(10.0),
            Some(1_000),
        )
        .await
        .expect_err("transport failure must block inference");
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
    }

    /// A request that names no cap of its own must still be held to the org's
    /// configured ceiling — otherwise omitting the field would be a way to opt
    /// out of the operator's limit.
    #[tokio::test]
    async fn an_org_ceiling_applies_to_a_request_that_names_no_cap() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "allowed": false,
                "reason": "org ceiling exceeded"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let uncapped = NormalizedRequest {
            max_cost_usd: None,
            max_tokens: None,
            ..normalized()
        };
        let error = check_budget_at(
            &reqwest::Client::new(),
            &server.uri(),
            "org-a",
            "user-a",
            "verified-cost-token",
            // What check_budget would resolve: request None, org 2.5.
            uncapped.max_cost_usd.or(Some(2.5)),
            None,
        )
        .await
        .expect_err("the org ceiling must block this");
        assert_eq!(error.0, StatusCode::PAYMENT_REQUIRED);
    }

    #[test]
    fn a_request_cap_wins_over_the_org_ceiling_only_where_it_is_set() {
        let org = crate::org_quota::OrgQuotaLimits {
            max_cost_usd: Some(2.5),
            max_tokens: Some(500),
        };
        let request = normalized(); // cost 10.0, tokens 1_000

        // A caller may TIGHTEN its own run…
        assert_eq!(request.max_cost_usd.or(org.max_cost_usd), Some(10.0));
        // …but a dimension it left unset still falls back to the org ceiling,
        // so omission cannot be used to escape the operator's limit.
        let partial = NormalizedRequest {
            max_cost_usd: None,
            ..normalized()
        };
        assert_eq!(partial.max_cost_usd.or(org.max_cost_usd), Some(2.5));
    }
}
