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

/// The cost-core base URL the gateway calls: `COST_CORE_URL`, or the local default.
pub(crate) fn cost_core_base_url() -> String {
    std::env::var(COST_CORE_BASE_URL_VAR).unwrap_or_else(|_| DEFAULT_COST_CORE_URL.to_owned())
}

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

    let base_url = cost_core_base_url();
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

/// Whether the org has any spend ceiling a key turn is checked against. Without one the key-turn
/// check makes no call (metering only), so no cost-core credential is needed either.
#[must_use]
pub(crate) fn org_has_ceiling(org_limits: &crate::org_quota::OrgQuotaLimits) -> bool {
    org_limits.max_cost_usd.is_some() || org_limits.max_tokens.is_some()
}

/// The gateway's service key variable, which `cost_budget_auth::from_env` also reads.
pub(crate) const SERVICE_API_KEY_ENV: &str = "MODEL_GATEWAY_SERVICE_API_KEY";

/// Whether the gateway has a cost-core service credential configured: `MODEL_GATEWAY_SERVICE_API_KEY`
/// is set and not blank, the same test `cost_budget_auth::from_env` applies. A half-configured
/// credential (for example one without `AUTH_CORE_URL`) still counts as configured, so its missing
/// bearer fails closed.
#[must_use]
pub(crate) fn service_credential_configured(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(SERVICE_API_KEY_ENV).is_some_and(|value| !value.trim().is_empty())
}

/// The cost-core credentials a key turn can be checked with, in the order they are tried.
///
/// With `service_configured` set, the service path is the only path: a service bearer that was not
/// minted fails the turn closed, and the key's own bearer is never sent (P2-mg-08-rel1). The key's
/// own bearer decides only when no service credential is configured at all (O-18).
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyBudgetCredentials<'a> {
    /// The gateway's own `aud=cost-core` service credential (`cost:budget:check`, F-f10-23a/b).
    /// Tried first. `None` means no bearer was minted: it is absent, blank, or the mint was refused.
    /// Once one is minted, its answer is final: a refusal or no answer decides the turn, and the
    /// key's bearer is not tried.
    pub service_bearer: Option<&'a str>,
    /// The key's own `aud=cost-core` bearer. Used only when no service credential is configured
    /// (`service_configured` is `false`), and present only when `COST_CORE_ACCEPT_API_KEY` is on
    /// (see [`key_budget_bearer_enabled`]). It sees only the key's own rows, so it never answers
    /// for the org.
    pub key_bearer: Option<&'a str>,
    /// Whether the gateway has a cost-core service credential configured. Defaults to `false`: none
    /// is configured, so the key's own bearer may decide. [`check_budget_for_key`] also sets it when
    /// `MODEL_GATEWAY_SERVICE_API_KEY` is set and not blank, so a caller can raise it, never lower it.
    pub service_configured: bool,
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
/// key's own spend: `user_id` is always `apikey:<key_id>`, whichever credential asks. The service
/// credential is tried first. Once it is minted, its answer decides: cost-core accepted it, so the
/// same credential also asks the org-wide question (`user_id` empty, F-f10-23b), which is required.
/// A refusal from either question is the typed `402 budget_exceeded`. A question that gets no
/// answer (401, 403, a server error, an unreadable body or a timeout) fails closed with
/// `503 budget_unavailable`, and the key's own bearer is not tried after a minted service
/// credential (P2-mg-08-rel1): that bearer sees only the key's rows, so its allowed answer would
/// admit an org over its org-wide ceiling.
///
/// A configured service credential (`credentials.service_configured`, or
/// `MODEL_GATEWAY_SERVICE_API_KEY` set and not blank) makes the service path the only path. When
/// its bearer was not minted, because it is absent, blank, or the mint was refused, the turn fails
/// closed with `503` and no request is made: the key's own bearer is never sent in that state
/// (P2-mg-08-rel1). Only when no service credential is configured does the key's own bearer decide
/// (O-18), and then it answers the key's own question alone. An org with no ceiling is never
/// checked, so it fails open (metering only), as [`check_budget`] does for users. The check carries
/// ids, a principal string and caps, no content, so it needs no ZDR branch; a ZDR key is refused
/// before this is reached.
///
/// # Errors
///
/// `402` over budget; `503` when the check cannot be decided for an org with a ceiling: a minted
/// service bearer got no answer on either question, a configured service credential was not
/// minted, or no service credential is configured and the key's own bearer is absent or got no
/// answer.
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
    if !org_has_ceiling(&org_limits) {
        return Ok(());
    }
    // P2-mg-08-rel1: a configured service key makes the service path the only path, even when the
    // caller did not say so. A caller can raise `service_configured`, never lower it.
    let credentials = KeyBudgetCredentials {
        service_configured: credentials.service_configured
            || service_credential_configured(&|key| std::env::var(key).ok()),
        ..credentials
    };
    let base_url = cost_core_base_url();
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

/// The key-turn decision, with the cost-core address and the windows passed in so the tests can point
/// it at a mock. The order is fixed: a minted service bearer decides first, and its answer is final;
/// a configured service credential with no usable bearer fails closed with no request; only with no
/// service credential configured does the key's own bearer decide.
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
    let scoped = |user_id: String| BudgetCheckRequest {
        org_id: org_id.to_owned(),
        user_id,
        max_cost_usd,
        max_tokens,
        since: windows.since.clone(),
        since_cost: windows.since_cost.clone(),
        since_tokens: windows.since_tokens.clone(),
    };
    let body = scoped(format!("apikey:{key_id}"));
    // F-f10-23b: the gateway's own credential asks about the key first and then, because an org-wide
    // ceiling is the one that matters for a key, about the whole org (`user_id` empty). Only this
    // path can ask the org-wide question: cost-core forces a key bearer to the key's own rows.
    if let Some(service_bearer) = usable_bearer(credentials.service_bearer) {
        match send_key_check(http_client, &url, service_bearer, &body).await {
            PathVerdict::Refused(refusal) => return Err(refusal),
            PathVerdict::Allowed => {
                // cost-core accepted the service credential, so an org-wide answer that does not
                // come is an outage, not a missing path: fail closed, never fall back to a check
                // that cannot see the org's spend.
                let org_wide = scoped(String::new());
                return match send_key_check(http_client, &url, service_bearer, &org_wide).await {
                    PathVerdict::Allowed => Ok(()),
                    PathVerdict::Refused(refusal) => Err(refusal),
                    PathVerdict::Unavailable => {
                        tracing::warn!(%org_id, "org-wide key turn budget check unanswered; blocking");
                        Err(dependency_unavailable("budget authority is unavailable"))
                    }
                };
            }
            // P2-mg-08-rel1: a minted service credential that cost-core did not answer is an outage,
            // not a missing path. The key's own bearer never takes over here: it sees only the key's
            // rows, so an allowed answer from it would admit an org over its org-wide ceiling.
            PathVerdict::Unavailable => {
                tracing::warn!(
                    %org_id,
                    "service-credential key turn budget check unanswered; blocking"
                );
                return Err(dependency_unavailable("budget authority is unavailable"));
            }
        }
    }
    // P2-mg-08-rel1: a configured service credential whose bearer was not minted is an outage, not
    // a missing path. The key's own bearer never decides in that state, so no request is made.
    if credentials.service_configured && usable_bearer(credentials.service_bearer).is_none() {
        tracing::warn!(
            %org_id,
            "the configured service credential was not minted; blocking the key turn budget check"
        );
        return Err(dependency_unavailable("budget authority is unavailable"));
    }
    // No service credential is configured (O-18): the key's own bearer is the only path left, and it
    // answers the key's own question alone.
    let verdict = match usable_bearer(credentials.key_bearer) {
        Some(key_bearer) => send_key_check(http_client, &url, key_bearer, &body).await,
        None => PathVerdict::Unavailable,
    };
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

/// The most ledger entries one run's list asks cost-core for: its own list maximum. A list that comes
/// back this full may have been cut short, so the thread export does not total it.
pub(crate) const RUN_ENTRIES_LIMIT: usize = 1000;

/// One ledger entry of a run, as cost-core lists it: the fields the thread export reads. The rest of
/// the entry is ignored.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RunLedgerEntry {
    #[serde(default)]
    pub(crate) model: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) created_at: DateTime<Utc>,
}

/// cost-core's answer to a run's entries list.
#[derive(Debug, Deserialize)]
struct RunEntriesResponse {
    entries: Vec<RunLedgerEntry>,
}

/// Lists one run's ledger entries as the viewer. The viewer's own `aud=cost-core` bearer is sent the
/// way the budget check sends it, under the same two second bound. cost-core pins a user token to its
/// own rows, so the list is the viewer's usage of that run and nothing else.
///
/// `None` when cost-core gives no list: no usable bearer, a refusal (an auth failure included), a
/// timeout, an unreachable service or a body that is not the list. The run id travels as a query
/// parameter, and the log names neither it nor the URL.
pub(crate) async fn list_run_entries(
    http_client: &reqwest::Client,
    base_url: &str,
    bearer: &str,
    run_id: &str,
) -> Option<Vec<RunLedgerEntry>> {
    let bearer = usable_bearer(Some(bearer))?;
    let run_id = run_id.trim();
    if run_id.is_empty() {
        return None;
    }
    let url = format!("{}/api/v1/cost/entries", base_url.trim_end_matches('/'));
    let limit = RUN_ENTRIES_LIMIT.to_string();
    let response = http_client
        .get(&url)
        .bearer_auth(bearer)
        .query(&[("run_id", run_id), ("limit", limit.as_str())])
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            match response.json::<RunEntriesResponse>().await {
                Ok(list) => Some(list.entries),
                Err(_) => {
                    tracing::warn!("cost-core returned an invalid run entries response");
                    None
                }
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "cost-core did not list a run's entries");
            None
        }
        Err(error) => {
            tracing::warn!(
                timeout = error.is_timeout(),
                "cost-core unreachable for a run's entries"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use wiremock::{
        matchers::{body_json, header, method, path, query_param},
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

    fn org_wide_body(cost: f64, tokens: u32) -> serde_json::Value {
        serde_json::json!({
            "org_id": "org-a",
            "user_id": "",
            "max_cost_usd": cost,
            "max_tokens": tokens,
        })
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
            .and(header("authorization", "Bearer service-credential"))
            .and(body_json(org_wide_body(10.0, 1_000)))
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
                service_configured: true,
            },
        )
        .await
        .expect("within budget on the service path; the key bearer is never used");
    }

    /// P2-mg-08-rel1: once the gateway's own credential is minted, a key-scope question it gets no
    /// answer to (401, 403, a server error, an unreadable body or a timeout) fails closed, and the
    /// key's own bearer is never sent. That bearer sees only the key's rows, so its allowed answer
    /// would admit an org over its org-wide ceiling, which the service credential was minted to check.
    #[tokio::test]
    async fn a_minted_service_bearer_without_an_answer_fails_closed() {
        let slow = || allowed().set_delay(std::time::Duration::from_secs(5));
        for key in [
            ResponseTemplate::new(401),
            ResponseTemplate::new(403),
            ResponseTemplate::new(500),
            ResponseTemplate::new(503),
            ResponseTemplate::new(200).set_body_string("not json"),
            slow(),
        ] {
            // The org-wide question would be allowed, so only the missing key-scope answer can fail it.
            let server = cost_core(key, allowed(), 0).await;
            let (status, Json(body)) = key_check(&server, BOTH)
                .await
                .expect_err("no answer on the service path: fail closed");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "budget_unavailable");
        }
    }

    /// The key's own bearer decides only when no service credential is configured (O-18): then it is
    /// the only path, and it answers the key's own question alone.
    #[tokio::test]
    async fn the_key_bearer_is_the_fallback_only_when_no_service_credential_is_configured() {
        // Not provisioned, a blank one, and one with whitespace in it: none is a minted bearer.
        for service in [None, Some("   "), Some("bad credential")] {
            let server = MockServer::start().await;
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
                    service_bearer: service,
                    key_bearer: Some("key-credential"),
                    service_configured: false,
                },
            )
            .await
            .expect("allowed on the key path alone");
        }
    }

    /// P2-mg-08-rel1: a configured service credential whose bearer was not minted (absent, blank, or
    /// with whitespace in it) fails closed without any request, and the key's own bearer is never
    /// sent, though it is present and usable. That is the state after the mint is refused, for
    /// example when Auth Core does not grant the gateway the cost-core audience, so the key's own
    /// answer must not stand in for the org-wide one.
    #[tokio::test]
    async fn a_configured_service_credential_that_is_not_minted_fails_closed() {
        // Any request at all is a failure: the service path is closed, and so is the key's bearer.
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        for service in [None, Some("   "), Some("bad credential")] {
            let (status, Json(body)) = key_check(
                &server,
                KeyBudgetCredentials {
                    service_bearer: service,
                    key_bearer: Some("key-credential"),
                    service_configured: true,
                },
            )
            .await
            .expect_err("a configured service credential that was not minted fails closed");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{service:?}");
            assert_eq!(body["error"], "budget_unavailable", "{service:?}");
        }
    }

    #[test]
    fn the_service_credential_is_configured_only_when_its_key_is_set_and_not_blank() {
        let set = "fixture-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(service_credential_configured(&|key| {
            (key == SERVICE_API_KEY_ENV).then(|| set.to_owned())
        }));
        for value in ["", "   ", "\t"] {
            assert!(
                !service_credential_configured(&|_| Some(value.to_owned())),
                "{value:?}"
            );
        }
        assert!(!service_credential_configured(&|_| None));
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
                service_configured: true,
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
                service_configured: false,
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
                service_configured: true,
            },
        ] {
            let (status, Json(body)) = key_check(&server, credentials)
                .await
                .expect_err("no credential, no decision");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "budget_unavailable");
        }

        // The minted service credential refused by cost-core's auth: no fallback, so the key's bearer
        // is never sent and the check fails closed.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer service-credential"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        let (status, _) = key_check(&server, BOTH).await.expect_err("fails closed");
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
                service_configured: false,
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
        assert!(!org_has_ceiling(
            &crate::org_quota::OrgQuotaLimits::default()
        ));
        assert!(org_has_ceiling(&crate::org_quota::OrgQuotaLimits {
            max_tokens: Some(1),
            ..Default::default()
        }));
    }

    // F-f10-23b: the gateway's own `cost:budget:check` credential, key scope then org-wide.

    /// cost-core answers the key-scope question with `key`, the org-wide one with `org`, and any
    /// request carrying the key's own bearer is counted (`key_bearer_calls`).
    async fn cost_core(
        key: ResponseTemplate,
        org: ResponseTemplate,
        key_bearer_calls: u64,
    ) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer service-credential"))
            .and(body_json(key_body(10.0, 1_000)))
            .respond_with(key)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer service-credential"))
            .and(body_json(org_wide_body(10.0, 1_000)))
            .respond_with(org)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(key_bearer_calls)
            .mount(&server)
            .await;
        server
    }

    const BOTH: KeyBudgetCredentials<'static> = KeyBudgetCredentials {
        service_bearer: Some("service-credential"),
        key_bearer: Some("key-credential"),
        service_configured: true,
    };

    #[tokio::test]
    async fn an_org_wide_refusal_is_final_and_the_key_bearer_is_never_sent() {
        let server = cost_core(allowed(), over_budget(), 0).await;
        let (status, Json(body)) = key_check(&server, BOTH).await.expect_err("org over budget");
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(body["error"], "budget_exceeded");
    }

    #[tokio::test]
    async fn an_org_wide_answer_that_does_not_come_fails_closed_without_the_key_bearer() {
        // 401 (an auth refusal), a server error and an unreadable body all fail closed once the
        // service credential was accepted: the key's bearer cannot see the org's spend.
        for org in [
            ResponseTemplate::new(401),
            ResponseTemplate::new(503),
            ResponseTemplate::new(200).set_body_string("not json"),
        ] {
            let server = cost_core(allowed(), org, 0).await;
            let (status, Json(body)) = key_check(&server, BOTH).await.expect_err("fails closed");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "budget_unavailable");
        }
    }

    #[tokio::test]
    async fn a_service_credential_cost_core_refuses_fails_closed_with_no_key_bearer() {
        let server = cost_core(ResponseTemplate::new(401), allowed(), 0).await;
        let (status, _) = key_check(
            &server,
            KeyBudgetCredentials {
                service_bearer: Some("service-credential"),
                key_bearer: None,
                service_configured: true,
            },
        )
        .await
        .expect_err("401 decides nothing: fail closed");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn a_cost_core_that_does_not_answer_in_time_fails_closed() {
        let slow = || allowed().set_delay(std::time::Duration::from_secs(5));
        // Slow on the key-scope question, and slow on the org-wide question after an allowed key. The
        // key's own bearer is present, and expects no call: a timed-out answer is no answer, not a path.
        for (key, org) in [(slow(), allowed()), (allowed(), slow())] {
            let server = cost_core(key, org, 0).await;
            let (status, Json(body)) = key_check(
                &server,
                KeyBudgetCredentials {
                    service_bearer: Some("service-credential"),
                    key_bearer: Some("key-credential"),
                    service_configured: true,
                },
            )
            .await
            .expect_err("timed out");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["error"], "budget_unavailable");
        }
    }

    #[tokio::test]
    async fn the_minted_service_bearer_is_reused_and_is_the_only_bearer_cost_core_sees() {
        use crate::cost_budget_auth::CostBudgetTokenProvider;
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/cost-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "service-credential",
                "expiresInSeconds": 300,
                "audience": "cost-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer service-credential"))
            .respond_with(allowed())
            .expect(4)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer key-credential"))
            .respond_with(allowed())
            .expect(0)
            .mount(&server)
            .await;
        let provider = CostBudgetTokenProvider::new_for_test(
            auth.uri(),
            "model-gateway",
            "fixture-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        // Two turns: each asks cost-core again (no verdict is cached), the bearer is minted once.
        for _ in 0..2 {
            let bearer = provider.budget_check_token("org-a").await.expect("minted");
            key_check(
                &server,
                KeyBudgetCredentials {
                    service_bearer: Some(&bearer),
                    key_bearer: Some("key-credential"),
                    service_configured: true,
                },
            )
            .await
            .expect("within budget for the key and the org");
        }
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
                    service_configured: true,
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

    #[tokio::test]
    async fn a_run_is_listed_as_the_viewer_for_that_run_up_to_the_list_maximum() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/cost/entries"))
            .and(query_param("run_id", "run-1"))
            .and(query_param("limit", "1000"))
            .and(header("authorization", "Bearer fixture-cost-bearer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": [{
                    "org_id": "org-a",
                    "user_id": "user-a",
                    "run_id": "run-1",
                    "model": "model-a",
                    "input_tokens": 10,
                    "output_tokens": 4,
                    "cost_usd": 0.01,
                    "created_at": "2026-11-20T10:00:00.123456789Z",
                    "cost_basis": "metered"
                }],
                "count": 1
            })))
            .expect(1)
            .mount(&server)
            .await;

        let entries = list_run_entries(
            &reqwest::Client::new(),
            &server.uri(),
            "fixture-cost-bearer",
            "run-1",
        )
        .await
        .expect("cost-core lists the run");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "model-a");
        assert_eq!((entries[0].input_tokens, entries[0].output_tokens), (10, 4));
    }

    #[tokio::test]
    async fn a_refused_garbled_or_late_run_list_is_no_list() {
        let server = MockServer::start().await;
        for (run_id, response) in [
            ("missing", ResponseTemplate::new(404)),
            ("refused", ResponseTemplate::new(401)),
            (
                "garbled",
                ResponseTemplate::new(200).set_body_string("not a list"),
            ),
            (
                "late",
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "entries": [], "count": 0 }))
                    .set_delay(std::time::Duration::from_secs(3)),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path("/api/v1/cost/entries"))
                .and(query_param("run_id", run_id))
                .respond_with(response)
                .mount(&server)
                .await;
        }

        let client = reqwest::Client::new();
        for run_id in ["missing", "refused", "garbled", "late"] {
            assert!(
                list_run_entries(&client, &server.uri(), "fixture-cost-bearer", run_id)
                    .await
                    .is_none(),
                "{run_id} keeps no list"
            );
        }
    }

    #[tokio::test]
    async fn no_run_list_is_asked_without_a_usable_bearer_or_a_run() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/cost/entries"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "entries": [] })),
            )
            .expect(0)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        assert!(list_run_entries(&client, &server.uri(), "", "run-1")
            .await
            .is_none());
        assert!(
            list_run_entries(&client, &server.uri(), "bad token", "run-1")
                .await
                .is_none()
        );
        assert!(
            list_run_entries(&client, &server.uri(), "fixture-cost-bearer", "  ")
                .await
                .is_none()
        );
    }
}
