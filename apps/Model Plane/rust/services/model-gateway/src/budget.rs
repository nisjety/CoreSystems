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
pub(crate) const BUDGET_WINDOW_ENV: &str = "MODEL_GATEWAY_BUDGET_WINDOW";
/// Switch for the per-run cap rule (D2-698). Only the literal `true` enables it.
/// On, a cap on the request can only tighten the org ceiling: the smaller of the
/// two applies. Off, a cap on the request replaces the org ceiling, as it did
/// before. The gate reads it per process through `cap_min_enabled`.
pub const CAP_MIN_ENV: &str = "MODEL_GATEWAY_CAP_MIN";
/// Switch for the per-cap windows (F-f02-04). With the windowed budget on, the cost cap and the token
/// cap are each measured over their own org-quota row's reset period, not the widest of the two. Only
/// the literal `true` enables it, read per request. cost-core is deployed before this switch goes on:
/// it refuses the `since_cost` and `since_tokens` fields it does not yet know.
pub(crate) const BUDGET_WINDOW_PER_CAP_ENV: &str = "MODEL_GATEWAY_BUDGET_WINDOW_PER_CAP";
/// Switch for the run token cap of a Do run (F-f04-08). Only the literal `true` enables it: on, the Do
/// run's request states the smaller of the request's and the org's metered-token ceilings, and a goal
/// turn's remaining allowance when that is smaller. Execution-core holds the run to it only while its
/// own switch is on, so the two can be turned on in either order.
pub(crate) const DO_RUN_CAP_ENV: &str = "MODEL_GATEWAY_DO_RUN_CAP";

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
    /// Start of the window the cost cap is measured over, when each cap has its own (F-f02-04). Absent
    /// measures it over `since`, so the field is not sent unless the per-cap switch is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    since_cost: Option<String>,
    /// Start of the window the token cap is measured over, as `since_cost` is for the cost cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    since_tokens: Option<String>,
}

/// The window or windows one budget check is measured over. With the per-cap switch off, `since` alone
/// (both caps share it, or neither has one). With it on, `since` is unset and each cap that is in force
/// names its own start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BudgetWindows {
    since: Option<String>,
    since_cost: Option<String>,
    since_tokens: Option<String>,
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
    let windows = budget_windows(
        budget_window_enabled(&|key| std::env::var(key).ok()),
        budget_window_per_cap_enabled(&|key| std::env::var(key).ok()),
        &org_limits,
        effective_cost,
        effective_tokens,
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
        windows,
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

/// Whether a Do run is stated a token cap (F-f04-08). Only the literal `true` enables it.
#[must_use]
pub(crate) fn do_run_cap_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(DO_RUN_CAP_ENV).as_deref() == Some("true")
}

/// The metered-token cap a Do run's request states (F-f04-08): the smaller of the request's cap and
/// the org's ceiling, then the smaller of that and a goal turn's remaining allowance. `None` when the
/// switch is off or no dimension is capped, which is the wire's `0`, no cap.
///
/// The request's cap never loosens the org's ceiling here, whatever `MODEL_GATEWAY_CAP_MIN` says,
/// because this figure is what execution-core holds the run to. A goal allowance of zero is held at
/// one token: `0` on the wire is no cap, and session-core refuses a claim with nothing left, so it
/// does not arrive here in practice.
#[must_use]
pub(crate) fn run_token_cap(
    switch_on: bool,
    request: Option<u32>,
    org: Option<i64>,
    goal: Option<u64>,
) -> Option<u64> {
    if !switch_on {
        return None;
    }
    let request = request.filter(|cap| *cap > 0).map(u64::from);
    let org = org
        .and_then(|cap| u64::try_from(cap).ok())
        .filter(|cap| *cap > 0);
    let goal = goal.map(|cap| cap.max(1));
    min_cap(min_cap(request, org), goal)
}

fn budget_window_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(BUDGET_WINDOW_ENV).as_deref() == Some("true")
}

/// Whether each cap is measured over its own window (F-f02-04). Only the literal `true` enables it,
/// and it has an effect only while the windowed budget itself is on.
fn budget_window_per_cap_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(BUDGET_WINDOW_PER_CAP_ENV).as_deref() == Some("true")
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

/// The windows one check is measured over (F-f02-04). Off, or with the windowed budget off, it is the
/// one `since` both caps share, exactly as before. With both switches on, the cost cap is measured over
/// its own row's period and the token cap over its own, each sent only when that cap is in force; the
/// shared `since` is then not sent at all.
fn budget_windows(
    window_on: bool,
    per_cap_on: bool,
    limits: &org_quota::OrgQuotaLimits,
    cost_cap: Option<f64>,
    token_cap: Option<u32>,
    now: DateTime<Utc>,
) -> BudgetWindows {
    if window_on && per_cap_on {
        return BudgetWindows {
            since: None,
            since_cost: cost_cap.and_then(|_| budget_window_since(true, limits.cost_period, now)),
            since_tokens: token_cap
                .and_then(|_| budget_window_since(true, limits.token_period, now)),
        };
    }
    BudgetWindows {
        since: budget_window_since(window_on, limits.reset_period, now),
        ..BudgetWindows::default()
    }
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
    windows: BudgetWindows,
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
        since: windows.since,
        since_cost: windows.since_cost,
        since_tokens: windows.since_tokens,
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

/// Switch for the Agent API key-turn budget check (O-18, F-f10-23b). Only the literal `true`
/// enables it, read per request. Off, a key turn is not budget-checked, as before.
pub const AGENT_API_BUDGET_CHECK_ENV: &str = "AGENT_API_BUDGET_CHECK";

/// cost-core's own switch for admitting an `api_key` principal on the budget check (O-18).
/// model-gateway reads the same literal only to decide whether the key-bearer fallback may be
/// used: with it off cost-core would refuse the key, so the key's bearer is never delegated.
pub const COST_CORE_ACCEPT_API_KEY_ENV: &str = "COST_CORE_ACCEPT_API_KEY";

/// Whether a key turn is budget-checked at all. Only the literal `true` enables it.
#[must_use]
pub fn agent_api_budget_check_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(AGENT_API_BUDGET_CHECK_ENV).as_deref() == Some("true")
}

/// Whether the key's own `aud=cost-core` bearer is accepted on the start route: the check is on
/// AND cost-core admits a key principal. Both are the exact literal `true`.
#[must_use]
pub fn key_budget_bearer_enabled() -> bool {
    let lookup = |key: &str| std::env::var(key).ok();
    agent_api_budget_check_enabled(&lookup)
        && lookup(COST_CORE_ACCEPT_API_KEY_ENV).as_deref() == Some("true")
}

/// The cost-core credentials a key turn can be checked with, in the order they are tried.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyBudgetCredentials<'a> {
    /// The gateway's own `aud=cost-core` service credential (`cost:budget:check`, F-f10-23a/b).
    /// Tried first. `None` while that credential is not provisioned.
    pub service_bearer: Option<&'a str>,
    /// The key's own `aud=cost-core` bearer. The fallback, only when the service path is
    /// unavailable, and present only when `COST_CORE_ACCEPT_API_KEY` is on (see
    /// [`key_budget_bearer_enabled`]).
    pub key_bearer: Option<&'a str>,
}

/// What one credential path said.
enum PathVerdict {
    Allowed,
    /// cost-core answered "over budget": final, the other path is never asked.
    Refused((StatusCode, Json<serde_json::Value>)),
    /// No credential, a transport failure, or an answer that is not a budget decision (an auth
    /// refusal, a server error, an unreadable body): the path decided nothing.
    Unavailable,
}

/// Check an Agent API key turn against the org's ceilings (O-18).
///
/// The ceilings are the org's (`org_limits`); a key turn carries no request cap. The scope is the
/// key's own spend: `user_id` is always `apikey:<key_id>`, whichever credential asks, so both
/// paths read the same ledger rows and agree. The service credential is tried first; the key's own
/// bearer only when the service path decided nothing. A refusal from either is the typed
/// `402 budget_exceeded`. Neither path answering fails closed (`503 budget_unavailable`) for an org
/// with a ceiling; an org with no ceiling is never checked, so it fails open (metering only), as
/// [`check_budget`] does for users. The check carries ids, a principal string and caps, no
/// content, so it needs no ZDR branch; a ZDR key is refused before this is reached.
///
/// # Errors
///
/// `402` over budget, `503` when no credential path could decide for an org with a ceiling.
pub async fn check_budget_for_key(
    http_client: &reqwest::Client,
    org_id: &str,
    key_id: &str,
    credentials: KeyBudgetCredentials<'_>,
    org_limits: crate::org_quota::OrgQuotaLimits,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // The quota column is i64, the wire field u32: clamp, never wrap (see `check_budget`).
    let tokens = org_limits
        .max_tokens
        .map(|tokens| u32::try_from(tokens).unwrap_or(u32::MAX));
    if org_limits.max_cost_usd.is_none() && tokens.is_none() {
        return Ok(());
    }
    let base_url =
        std::env::var(COST_CORE_BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_COST_CORE_URL.to_owned());
    let windows = budget_windows(
        budget_window_enabled(&|key| std::env::var(key).ok()),
        budget_window_per_cap_enabled(&|key| std::env::var(key).ok()),
        &org_limits,
        org_limits.max_cost_usd,
        tokens,
        Utc::now(),
    );
    check_budget_for_key_at(
        http_client,
        &base_url,
        org_id,
        key_id,
        credentials,
        org_limits.max_cost_usd,
        tokens,
        windows,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn check_budget_for_key_at(
    http_client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    key_id: &str,
    credentials: KeyBudgetCredentials<'_>,
    max_cost_usd: Option<f64>,
    max_tokens: Option<u32>,
    windows: BudgetWindows,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if org_id.trim().is_empty()
        || key_id.trim().is_empty()
        || key_id.chars().any(char::is_whitespace)
    {
        return Err(dependency_unavailable(
            "verified budget identity is unavailable",
        ));
    }
    let url = format!("{}/api/v1/budget/check", base_url.trim_end_matches('/'));
    let body = BudgetCheckRequest {
        org_id: org_id.to_owned(),
        user_id: format!("apikey:{key_id}"),
        max_cost_usd,
        max_tokens,
        since: windows.since,
        since_cost: windows.since_cost,
        since_tokens: windows.since_tokens,
    };
    let mut verdict = PathVerdict::Unavailable;
    for bearer in [credentials.service_bearer, credentials.key_bearer] {
        let Some(bearer) = usable_bearer(bearer) else {
            continue;
        };
        verdict = send_key_check(http_client, &url, bearer, &body).await;
        if !matches!(verdict, PathVerdict::Unavailable) {
            break;
        }
    }
    match verdict {
        PathVerdict::Allowed => Ok(()),
        PathVerdict::Refused(refusal) => Err(refusal),
        PathVerdict::Unavailable => {
            tracing::warn!(%org_id, "no credential path could check the key turn budget; blocking");
            Err(dependency_unavailable("budget authority is unavailable"))
        }
    }
}

/// A bearer that can be put in an Authorization header: present, non-empty, no whitespace.
fn usable_bearer(bearer: Option<&str>) -> Option<&str> {
    bearer
        .map(str::trim)
        .filter(|bearer| !bearer.is_empty() && !bearer.chars().any(char::is_whitespace))
}

async fn send_key_check(
    http_client: &reqwest::Client,
    url: &str,
    bearer: &str,
    body: &BudgetCheckRequest,
) -> PathVerdict {
    let response = http_client
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            match response.json::<BudgetCheckResponse>().await {
                Ok(check) if check.allowed => PathVerdict::Allowed,
                Ok(check) => {
                    tracing::warn!(
                        current_cost = ?check.current_cost_usd,
                        current_tokens = ?check.current_tokens,
                        "budget guard rejected key turn"
                    );
                    PathVerdict::Refused((
                        StatusCode::PAYMENT_REQUIRED,
                        Json(json!({
                            "error": "budget_exceeded",
                            "message": check
                                .reason
                                .unwrap_or_else(|| "budget exceeded".to_owned()),
                            "current_cost_usd": check.current_cost_usd,
                            "current_tokens": check.current_tokens,
                        })),
                    ))
                }
                Err(error) => {
                    tracing::warn!(%error, "cost-core returned an invalid key budget response");
                    PathVerdict::Unavailable
                }
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "cost-core did not decide the key budget check");
            PathVerdict::Unavailable
        }
        Err(error) => {
            tracing::warn!(%error, "cost-core unreachable for the key budget check");
            PathVerdict::Unavailable
        }
    }
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
            BudgetWindows::default(),
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
            BudgetWindows::default(),
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
            BudgetWindows::default(),
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
            BudgetWindows::default(),
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
            BudgetWindows {
                since,
                ..BudgetWindows::default()
            },
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

    // O-18: the key-turn budget check.

    fn key_body(cost: f64, tokens: u32) -> serde_json::Value {
        serde_json::json!({
            "org_id": "org-a",
            "user_id": "apikey:key-1",
            "max_cost_usd": cost,
            "max_tokens": tokens,
        })
    }

    fn allowed() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowed": true,
            "current_cost_usd": 0.5,
            "current_tokens": 10,
        }))
    }

    fn over_budget() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowed": false,
            "reason": "cost budget exceeded",
            "current_cost_usd": 12.0,
            "current_tokens": 2000,
        }))
    }

    async fn key_check(
        server: &MockServer,
        credentials: KeyBudgetCredentials<'_>,
    ) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        check_budget_for_key_at(
            &reqwest::Client::new(),
            &server.uri(),
            "org-a",
            "key-1",
            credentials,
            Some(10.0),
            Some(1_000),
            BudgetWindows::default(),
        )
        .await
    }

    #[test]
    fn the_key_budget_switch_is_the_strict_literal() {
        assert!(agent_api_budget_check_enabled(&|key| {
            (key == AGENT_API_BUDGET_CHECK_ENV).then(|| "true".to_owned())
        }));
        for value in ["True", "TRUE", "1", "yes", " true", "true ", ""] {
            assert!(
                !agent_api_budget_check_enabled(&|_| Some(value.to_owned())),
                "{value:?}"
            );
        }
        assert!(!agent_api_budget_check_enabled(&|_| None));
    }

    #[tokio::test]
    async fn a_key_turn_is_checked_for_its_own_principal_with_the_service_credential_first() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer service-credential"))
            .and(body_json(key_body(10.0, 1_000)))
            .respond_with(allowed())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;

        key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: Some("service-credential"),
                key_bearer: Some("key-credential"),
            },
        )
        .await
        .expect("within budget on the service path; the key bearer is never used");
    }

    #[tokio::test]
    async fn the_key_bearer_is_the_fallback_only_when_the_service_path_decided_nothing() {
        for service in [
            ResponseTemplate::new(401),
            ResponseTemplate::new(403),
            ResponseTemplate::new(503),
            ResponseTemplate::new(200).set_body_string("not json"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(header("authorization", "Bearer service-credential"))
                .respond_with(service)
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/api/v1/budget/check"))
                .and(header("authorization", "Bearer key-credential"))
                .and(body_json(key_body(10.0, 1_000)))
                .respond_with(allowed())
                .expect(1)
                .mount(&server)
                .await;
            key_check(
                &server,
                KeyBudgetCredentials {
                    service_bearer: Some("service-credential"),
                    key_bearer: Some("key-credential"),
                },
            )
            .await
            .expect("the key's own bearer decides when the service path did not");
        }

        // No service credential provisioned: the key path is the only one.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(1)
            .mount(&server)
            .await;
        key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: None,
                key_bearer: Some("key-credential"),
            },
        )
        .await
        .expect("allowed on the key path alone");
    }

    #[tokio::test]
    async fn an_over_budget_answer_is_final_and_typed_on_either_path() {
        // Service path refuses: the key path is never asked.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer service-credential"))
            .respond_with(over_budget())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        let (status, Json(via_service)) = key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: Some("service-credential"),
                key_bearer: Some("key-credential"),
            },
        )
        .await
        .expect_err("over budget");
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(via_service["error"], "budget_exceeded");
        assert_eq!(via_service["message"], "cost budget exceeded");

        // Key path refuses (no service credential): the same typed refusal, so both paths agree.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(over_budget())
            .expect(1)
            .mount(&server)
            .await;
        let (status, Json(via_key)) = key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: None,
                key_bearer: Some("key-credential"),
            },
        )
        .await
        .expect_err("over budget");
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(
            via_key, via_service,
            "both paths give the same typed refusal"
        );
    }

    #[tokio::test]
    async fn no_path_that_decides_fails_closed_for_an_org_with_a_ceiling() {
        // Nothing usable provisioned: no request is made.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        for credentials in [
            KeyBudgetCredentials::default(),
            KeyBudgetCredentials {
                service_bearer: Some("   "),
                key_bearer: Some("bad credential"),
            },
        ] {
            let (status, Json(body)) = key_check(&server, credentials)
                .await
                .expect_err("no credential, no decision");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "budget_unavailable");
        }

        // Both paths refused by cost-core's auth.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .expect(2)
            .mount(&server)
            .await;
        let (status, _) = key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: Some("service-credential"),
                key_bearer: Some("key-credential"),
            },
        )
        .await
        .expect_err("fails closed");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        // Unreachable cost-core.
        let (status, Json(body)) = check_budget_for_key_at(
            &reqwest::Client::new(),
            "http://127.0.0.1:1",
            "org-a",
            "key-1",
            KeyBudgetCredentials {
                service_bearer: None,
                key_bearer: Some("key-credential"),
            },
            Some(10.0),
            None,
            BudgetWindows::default(),
        )
        .await
        .expect_err("unreachable");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "budget_unavailable");
    }

    #[tokio::test]
    async fn an_org_with_no_ceiling_is_never_checked_and_fails_open() {
        // `check_budget_for_key` returns before it reads the cost-core address or any credential.
        check_budget_for_key(
            &reqwest::Client::new(),
            "org-a",
            "key-1",
            KeyBudgetCredentials::default(),
            crate::org_quota::OrgQuotaLimits::default(),
        )
        .await
        .expect("no ceiling, nothing to enforce: metering only");
    }

    #[tokio::test]
    async fn a_key_id_that_is_not_one_token_is_never_sent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        for key_id in ["", " ", "key 1"] {
            let outcome = check_budget_for_key_at(
                &reqwest::Client::new(),
                &server.uri(),
                "org-a",
                key_id,
                KeyBudgetCredentials {
                    service_bearer: Some("service-credential"),
                    key_bearer: None,
                },
                Some(10.0),
                None,
                BudgetWindows::default(),
            )
            .await;
            assert_eq!(
                outcome.expect_err("refused").0,
                StatusCode::SERVICE_UNAVAILABLE,
                "{key_id:?}"
            );
        }
    }

    // F-f02-04: each cap is measured over its own window.

    #[test]
    fn the_per_cap_switch_is_the_strict_literal() {
        assert!(budget_window_per_cap_enabled(&|key| {
            (key == BUDGET_WINDOW_PER_CAP_ENV).then(|| "true".to_owned())
        }));
        for value in ["True", "TRUE", "1", "yes", " true", "true ", ""] {
            assert!(
                !budget_window_per_cap_enabled(&|_| Some(value.to_owned())),
                "{value:?}"
            );
        }
        assert!(!budget_window_per_cap_enabled(&|_| None));
    }

    #[test]
    fn per_cap_windows_are_named_only_with_both_switches_and_only_for_a_cap_in_force() {
        let now = Utc
            .with_ymd_and_hms(2026, 10, 23, 15, 42, 10)
            .single()
            .expect("a valid instant");
        let limits = org_quota::OrgQuotaLimits {
            max_cost_usd: Some(2.5),
            max_tokens: Some(500),
            reset_period: ResetPeriod::Daily,
            cost_period: ResetPeriod::Daily,
            token_period: ResetPeriod::None,
        };
        let today = "2026-10-23T00:00:00Z";

        // Both switches on: the cost cap names its own day; the token row has no period, so it names none.
        assert_eq!(
            budget_windows(true, true, &limits, Some(2.5), Some(500), now),
            BudgetWindows {
                since: None,
                since_cost: Some(today.to_owned()),
                since_tokens: None,
            }
        );
        // A cap that is not in force names no window at all.
        assert_eq!(
            budget_windows(true, true, &limits, None, Some(500), now),
            BudgetWindows::default()
        );
        // The per-cap switch alone does nothing: the windowed budget is off, so nothing is windowed.
        assert_eq!(
            budget_windows(false, true, &limits, Some(2.5), Some(500), now),
            BudgetWindows::default()
        );
        // The windowed budget alone is as before: one shared start, from the widest period.
        assert_eq!(
            budget_windows(true, false, &limits, Some(2.5), Some(500), now),
            BudgetWindows {
                since: Some(today.to_owned()),
                since_cost: None,
                since_tokens: None,
            }
        );
    }

    #[tokio::test]
    async fn the_body_carries_the_per_cap_windows_and_no_shared_since_when_switched_on() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(body_json(serde_json::json!({
                "org_id": "org-a",
                "user_id": "user-a",
                "max_cost_usd": 10.0,
                "max_tokens": 1000,
                "since_cost": "2026-10-23T00:00:00Z",
                "since_tokens": "2026-10-01T00:00:00Z",
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
            BudgetWindows {
                since: None,
                since_cost: Some("2026-10-23T00:00:00Z".to_owned()),
                since_tokens: Some("2026-10-01T00:00:00Z".to_owned()),
            },
        )
        .await
        .expect("within both windows");
    }

    #[tokio::test]
    async fn the_body_is_unchanged_with_the_per_cap_switch_off() {
        // No window fields at all: the body is exactly the one a gateway sent before they existed.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(body_json(serde_json::json!({
                "org_id": "org-a",
                "user_id": "user-a",
                "max_cost_usd": 10.0,
                "max_tokens": 1000,
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
            BudgetWindows::default(),
        )
        .await
        .expect("within budget");
    }

    // F-f04-08: the run token cap a Do run states.

    #[test]
    fn the_run_token_cap_is_stated_only_with_the_switch_on() {
        assert_eq!(
            run_token_cap(false, Some(1_000), Some(500), Some(200)),
            None
        );
        assert!(do_run_cap_enabled(&|key| {
            (key == DO_RUN_CAP_ENV).then(|| "true".to_owned())
        }));
        for value in ["True", "TRUE", "1", "yes", " true", "true ", ""] {
            assert!(
                !do_run_cap_enabled(&|_| Some(value.to_owned())),
                "{value:?}"
            );
        }
        assert!(!do_run_cap_enabled(&|_| None));
    }

    #[test]
    fn the_smaller_of_the_request_and_the_org_ceiling_is_stated() {
        assert_eq!(run_token_cap(true, Some(800), Some(500), None), Some(500));
        assert_eq!(run_token_cap(true, Some(300), Some(500), None), Some(300));
        // The request cannot loosen the org ceiling, whatever MODEL_GATEWAY_CAP_MIN says.
        assert_eq!(
            run_token_cap(true, Some(u32::MAX), Some(1_000), None),
            Some(1_000)
        );
        // One side set is that one; neither set is no cap.
        assert_eq!(run_token_cap(true, None, Some(500), None), Some(500));
        assert_eq!(run_token_cap(true, Some(300), None, None), Some(300));
        assert_eq!(run_token_cap(true, None, None, None), None);
        // A cap that is not positive is no cap on that side, so the other still binds.
        assert_eq!(run_token_cap(true, Some(0), Some(500), None), Some(500));
        assert_eq!(run_token_cap(true, Some(300), Some(0), None), Some(300));
        assert_eq!(run_token_cap(true, Some(300), Some(-5), None), Some(300));
    }

    #[test]
    fn a_goal_turns_remaining_allowance_wins_when_it_is_the_smaller() {
        assert_eq!(
            run_token_cap(true, Some(800), Some(500), Some(120)),
            Some(120)
        );
        assert_eq!(
            run_token_cap(true, Some(800), Some(500), Some(900)),
            Some(500)
        );
        assert_eq!(run_token_cap(true, None, None, Some(120)), Some(120));
        // A goal allowance of zero is never the wire's 0 (no cap): it is held at one token.
        assert_eq!(run_token_cap(true, Some(800), Some(500), Some(0)), Some(1));
    }
}
