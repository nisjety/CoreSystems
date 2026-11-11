//! Budget guard — pre-flight check against cost-core before forwarding to inference.
//!
//! When `max_cost_usd` or `max_tokens` are set on the request, the gateway queries
//! cost-core for current usage and rejects the request if the budget would be exceeded.

use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::normalize::NormalizedRequest;
use crate::org_quota::{self, ResetPeriod};

const COST_CORE_BASE_URL_VAR: &str = "COST_CORE_URL";
const DEFAULT_COST_CORE_URL: &str = "http://localhost:8089";
/// Switch for the windowed org budget (WS27-S8). Only the literal `true`
/// enables it, and it is read per request, so a restart is not needed to turn
/// it off. Off, the check counts lifetime spend, as it did before.
const BUDGET_WINDOW_ENV: &str = "MODEL_GATEWAY_BUDGET_WINDOW";
/// Switch for the per-run cap rule (D2-698). Only the literal `true` enables it.
/// On, a cap on the request can only tighten the org ceiling: the smaller of the
/// two applies. Off, a cap on the request replaces the org ceiling, as it did
/// before. The gate reads it per process through `cap_min_enabled`.
pub const CAP_MIN_ENV: &str = "MODEL_GATEWAY_CAP_MIN";

#[derive(Debug, Serialize)]
struct BudgetCheckRequest {
    org_id: String,
    user_id: String,
    max_cost_usd: Option<f64>,
    max_tokens: Option<u32>,
    /// Start of the org's current reset window (RFC 3339, UTC). Absent counts
    /// lifetime spend, so the field is not sent when the window is off.
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
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
/// (`org_quota`). With `MODEL_GATEWAY_CAP_MIN` on, a cap supplied on the REQUEST
/// can only tighten the org ceiling: the smaller of the two applies, and the org
/// ceiling still applies to any dimension the request left unset. With it off, a
/// request cap wins over the org default, as it did before.
pub async fn check_budget(
    http_client: &reqwest::Client,
    org_id: &str,
    user_id: &str,
    verified_bearer: &str,
    normalized: &NormalizedRequest,
    org_limits: crate::org_quota::OrgQuotaLimits,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let cap_min = cap_min_enabled(&|key| std::env::var(key).ok());
    let effective_cost = if cap_min {
        min_cap(normalized.max_cost_usd, org_limits.max_cost_usd)
    } else {
        normalized.max_cost_usd.or(org_limits.max_cost_usd)
    };
    // The quota column is i64; the wire field is u32. Clamp rather than wrap:
    // a ceiling larger than u32 is effectively "no limit", and wrapping would
    // turn a huge cap into a tiny one and reject everything.
    let org_tokens = org_limits
        .max_tokens
        .map(|tokens| u32::try_from(tokens).unwrap_or(u32::MAX));
    let effective_tokens = if cap_min {
        min_cap(normalized.max_tokens, org_tokens)
    } else {
        normalized.max_tokens.or(org_tokens)
    };
    if effective_cost.is_none() && effective_tokens.is_none() {
        return Ok(());
    }

    let base_url =
        std::env::var(COST_CORE_BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_COST_CORE_URL.to_owned());
    let since = budget_window_since(
        budget_window_enabled(&|key| std::env::var(key).ok()),
        org_limits.reset_period,
        Utc::now(),
    );
    check_budget_at(
        http_client,
        &base_url,
        org_id,
        user_id,
        verified_bearer,
        effective_cost,
        effective_tokens,
        since,
    )
    .await
}

/// Whether the per-run cap rule switch is on. Only the literal `true` enables it.
#[must_use]
pub fn cap_min_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(CAP_MIN_ENV).as_deref() == Some("true")
}

/// The tighter of a request cap and an org ceiling. When only one is set, that
/// one applies, so a request cannot loosen the org ceiling and the ceiling still
/// binds a request that set none. A value that does not compare (NaN) yields the
/// org ceiling, so an unreadable request cap cannot loosen it.
#[must_use]
pub fn min_cap<T: PartialOrd + Copy>(request: Option<T>, org: Option<T>) -> Option<T> {
    match (request, org) {
        (Some(request), Some(org)) => Some(if request <= org { request } else { org }),
        (request, org) => request.or(org),
    }
}

fn budget_window_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(BUDGET_WINDOW_ENV).as_deref() == Some("true")
}

/// The `since` to send with a check: the start of the org's current reset
/// window in UTC. `None` when the switch is off or the period is a lifetime
/// total, so the check then counts lifetime spend.
fn budget_window_since(enabled: bool, period: ResetPeriod, now: DateTime<Utc>) -> Option<String> {
    if !enabled {
        return None;
    }
    org_quota::window_since(period, now)
        .map(|start| start.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[allow(clippy::too_many_arguments)]
async fn check_budget_at(
    http_client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    user_id: &str,
    verified_bearer: &str,
    max_cost_usd: Option<f64>,
    max_tokens: Option<u32>,
    since: Option<String>,
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
        since,
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
    use chrono::TimeZone;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn normalized() -> NormalizedRequest {
        NormalizedRequest {
            content: "hello".to_owned(),
            model: String::new(),
            provider_hint: String::new(),
            subscription_connection_id: String::new(),
            model_connection_id: String::new(),
            session_key: None,
            thread_id: None,
            space_context: None,
            space_append_context: None,
            structured_output_schema: None,
            zdr: true,
            min_privacy_tier: None,
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
            None,
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
            None,
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
            None,
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
            ..Default::default()
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

    #[tokio::test]
    async fn reset_period_daily_sends_start_of_utc_day() {
        let now = Utc
            .with_ymd_and_hms(2026, 10, 23, 15, 42, 10)
            .single()
            .expect("a valid instant");
        let since = budget_window_since(true, ResetPeriod::Daily, now);
        assert_eq!(since.as_deref(), Some("2026-10-23T00:00:00Z"));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(body_json(serde_json::json!({
                "org_id": "org-a",
                "user_id": "user-a",
                "max_cost_usd": 10.0,
                "max_tokens": 1000,
                "since": "2026-10-23T00:00:00Z",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "allowed": true,
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
            since,
        )
        .await
        .expect("within the window");
    }

    #[test]
    fn the_window_is_sent_only_when_switched_on_and_the_period_is_not_lifetime() {
        let now = Utc
            .with_ymd_and_hms(2026, 10, 23, 15, 42, 10)
            .single()
            .expect("a valid instant");
        assert_eq!(budget_window_since(false, ResetPeriod::Daily, now), None);
        assert_eq!(budget_window_since(true, ResetPeriod::None, now), None);
        assert_eq!(
            budget_window_since(true, ResetPeriod::Monthly, now).as_deref(),
            Some("2026-10-01T00:00:00Z")
        );

        // Only the literal `true` enables it: a misspelling is off, never half-on.
        assert!(budget_window_enabled(&|_| Some("true".to_owned())));
        assert!(!budget_window_enabled(&|_| Some("True".to_owned())));
        assert!(!budget_window_enabled(&|_| None));
    }
}
