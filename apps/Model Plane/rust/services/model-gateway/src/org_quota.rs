//! Per-org spend/token ceilings, read from Control Plane.
//!
//! `CLAUDE.md`: "Control Plane owns identity, users, orgs, billing, sessions,
//! audit, **quotas**, and entitlements." So the ceiling lives in org-core
//! (`org_quotas`), not here. The gateway reads it and hands it to cost-core's
//! `/api/v1/budget/check`, which accounts actual spend against it.
//!
//! That split is deliberate rather than a workaround: cost-core's check takes
//! the cap **as a request parameter**, so it does accounting and the caller
//! supplies policy. This module is the "supplies policy" half — previously
//! missing, which is why an operator's cap enforced nothing.
//!
//! ## Units
//!
//! `org_quotas.quota_limit` is `BIGINT`, so a USD ceiling is stored in
//! **micro-dollars** under a key that says so. Storing dollars as a float in an
//! integer column would silently truncate; naming the key without the unit
//! would invite someone to write `5` meaning five dollars and get five
//! micro-dollars.

use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;

/// Quota key for the per-run USD ceiling, in micro-dollars (1 USD = 1e6).
pub const MAX_COST_PER_RUN_USD_MICROS: &str = "max_cost_per_run_usd_micros";
/// Quota key for the per-run token ceiling.
pub const MAX_TOKENS_PER_RUN: &str = "max_tokens_per_run";

/// One micro-dollar as a fraction of a dollar.
const MICROS_PER_USD: f64 = 1_000_000.0;

/// Convert a USD amount to the integer micro-dollars the quota column stores.
/// Rounds rather than truncates so `0.10` does not become `99_999` micros.
#[must_use]
pub fn usd_to_micros(usd: f64) -> i64 {
    (usd * MICROS_PER_USD).round() as i64
}

/// Convert stored micro-dollars back to USD.
#[must_use]
pub fn micros_to_usd(micros: i64) -> f64 {
    micros as f64 / MICROS_PER_USD
}

/// How often an org-core quota row's spend restarts (`reset_period`). An
/// unrecognised value reads as `None`, the lifetime total, which is what
/// cost-core already checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResetPeriod {
    Daily,
    Monthly,
    #[default]
    None,
}

impl ResetPeriod {
    fn parse(raw: &str) -> Self {
        match raw.trim() {
            "daily" => Self::Daily,
            "monthly" => Self::Monthly,
            _ => Self::None,
        }
    }

    /// The period whose window reaches further back, so a shared window never
    /// counts less spend than either cap's own period. Lifetime covers monthly,
    /// monthly covers daily.
    fn widest(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, _) | (_, Self::None) => Self::None,
            (Self::Monthly, _) | (_, Self::Monthly) => Self::Monthly,
            _ => Self::Daily,
        }
    }
}

/// The ceilings an org has configured. `None` means "no ceiling set", which is
/// different from a ceiling of zero — zero would forbid everything.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OrgQuotaLimits {
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<i64>,
    /// The widest reset period among the ceilings that are set. Both caps are
    /// checked against one window, so the window must reach back far enough
    /// that neither cap is measured over less spend than its own period names.
    pub reset_period: ResetPeriod,
}

impl OrgQuotaLimits {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.max_cost_usd.is_none() && self.max_tokens.is_none()
    }
}

#[derive(Debug, Deserialize)]
struct QuotaRow {
    key: String,
    limit: i64,
    #[serde(default)]
    reset_period: String,
}

#[derive(Debug, Deserialize)]
struct QuotasResponse {
    #[serde(default)]
    quotas: Vec<QuotaRow>,
}

/// Pick the run ceilings out of an org-core quotas payload.
///
/// A limit of 0 or less is treated as "not set". org-core accepts 0 as a
/// legitimate ceiling, but forwarding it here would hand cost-core a cap that
/// rejects every request — and an org whose chat silently stops working is a
/// worse failure than an unenforced cap. If "block everything" is ever wanted,
/// it should be an explicit disable, not a quota of zero.
#[must_use]
pub fn limits_from_rows(body: &str) -> OrgQuotaLimits {
    let Ok(parsed) = serde_json::from_str::<QuotasResponse>(body) else {
        return OrgQuotaLimits::default();
    };
    let find = |key: &str| {
        parsed
            .quotas
            .iter()
            .find(|row| row.key == key)
            .filter(|row| row.limit > 0)
    };
    let cost = find(MAX_COST_PER_RUN_USD_MICROS);
    let tokens = find(MAX_TOKENS_PER_RUN);
    OrgQuotaLimits {
        max_cost_usd: cost.map(|row| micros_to_usd(row.limit)),
        max_tokens: tokens.map(|row| row.limit),
        reset_period: match (cost, tokens) {
            (Some(cost), Some(tokens)) => ResetPeriod::parse(&cost.reset_period)
                .widest(ResetPeriod::parse(&tokens.reset_period)),
            (Some(row), None) | (None, Some(row)) => ResetPeriod::parse(&row.reset_period),
            (None, None) => ResetPeriod::None,
        },
    }
}

/// Start of the calendar window `period` names, in UTC, or `None` for a
/// lifetime total. Daily starts at midnight UTC today, monthly at midnight UTC
/// on the first of the month.
#[must_use]
pub fn window_since(period: ResetPeriod, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let day = match period {
        ResetPeriod::None => return None,
        ResetPeriod::Daily => now.date_naive(),
        ResetPeriod::Monthly => now.date_naive().with_day(1)?,
    };
    Some(day.and_hms_opt(0, 0, 0)?.and_utc())
}

/// Read an org's configured ceilings from org-core.
///
/// Authenticates with a SERVICE credential (`x-service-id`/`x-service-token`),
/// the same shape execution-core's `OrgDirectoryClient` uses. Forwarding the
/// chat caller's own token would not work: the tokens in hand on the invoke
/// path are Model Plane audience tokens (cost/inference/session), and none of
/// them carries an org-core scope — the read would 401 and, because this fails
/// open, the ceiling would silently stop applying.
///
/// **Fails OPEN**, deliberately, and says so in the log. If Control Plane is
/// unreachable the alternative is rejecting every chat turn in the product for
/// the duration of an unrelated outage. Spend is still accounted by cost-core,
/// and a caller may still supply per-request caps, so failing open loses the
/// org-configured ceiling rather than all protection. That trade is worth
/// revisiting if caps ever become a hard contractual limit rather than a guard.
pub async fn fetch_org_limits(
    client: &reqwest::Client,
    org_core_base_url: &str,
    org_id: &str,
    service_id: &str,
    service_token: &str,
) -> OrgQuotaLimits {
    if org_core_base_url.trim().is_empty() || org_id.trim().is_empty() {
        return OrgQuotaLimits::default();
    }
    if service_token.trim().is_empty() {
        // Unconfigured, not broken: idle quietly rather than logging a warning
        // on every turn, matching execution-core's convention for an
        // unconfigured Control Plane dependency.
        tracing::debug!(%org_id, "org-core service token unset; no org ceiling applied");
        return OrgQuotaLimits::default();
    }
    let url = format!(
        "{}/api/v1/organizations/{org_id}/quotas",
        org_core_base_url.trim_end_matches('/')
    );
    let request = client
        .get(&url)
        .header("x-service-id", service_id)
        .header("x-service-token", service_token);
    match request.send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(body) => limits_from_rows(&body),
            Err(error) => {
                tracing::warn!(%error, %org_id, "org quota body unreadable; proceeding uncapped");
                OrgQuotaLimits::default()
            }
        },
        Ok(response) => {
            tracing::warn!(
                status = %response.status(), %org_id,
                "org-core rejected the quota read; proceeding uncapped"
            );
            OrgQuotaLimits::default()
        }
        Err(error) => {
            tracing::warn!(%error, %org_id, "org-core unreachable; proceeding uncapped");
            OrgQuotaLimits::default()
        }
    }
}

/// Quota key for the browser planner's per-run token ceiling (WS11.S1). Its limit is a token
/// count. Deliberately NOT [`MAX_TOKENS_PER_RUN`]: that key bounds a chat run's spend through
/// cost-core, this one bounds only the LLM planner of a browser run.
pub const BROWSER_AGENT_MAX_TOKENS_PER_RUN: &str = "browser_agent_max_tokens_per_run";
/// Quota key for the org's LLM-planner switch (WS11.S2): limit 1 = on, 2 = off, anything else =
/// unset. A quota row is the interim carrier; the final one is a decision for Control Plane.
pub const BROWSER_AGENT_LLM_PLANNER: &str = "browser_agent_llm_planner";

/// What one read of an org's quota rows came back with, for a caller that needs more than the
/// two run ceilings of [`OrgQuotaLimits`]. `Unavailable` is a read that did not produce rows
/// (unconfigured, unreachable, refused, unreadable body); an org with no rows at all is
/// `Rows(vec![])`. The two are kept apart because the callers fail differently: a token ceiling
/// fails OPEN (an unreadable ceiling is no ceiling) and the planner switch fails CLOSED.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgQuotaKeys {
    Rows(Vec<(String, i64)>),
    Unavailable,
}

impl OrgQuotaKeys {
    /// Parse an org-core quotas payload. A body that is not the expected shape is `Unavailable`,
    /// not an empty org: an empty answer is a decision, a garbled one is not.
    #[must_use]
    pub fn from_body(body: &str) -> Self {
        match serde_json::from_str::<QuotasResponse>(body) {
            Ok(parsed) => Self::Rows(
                parsed
                    .quotas
                    .into_iter()
                    .map(|row| (row.key, row.limit))
                    .collect(),
            ),
            Err(_) => Self::Unavailable,
        }
    }

    /// The limit stored under `key`, if the rows were read and the key is present.
    #[must_use]
    pub fn limit(&self, key: &str) -> Option<i64> {
        match self {
            Self::Rows(rows) => rows
                .iter()
                .find(|(row_key, _)| row_key == key)
                .map(|(_, limit)| *limit),
            Self::Unavailable => None,
        }
    }

    /// The org's browser planner token ceiling. A limit of 0 or less is "not set", as for the
    /// other run ceilings; an unreadable read is "not set" too (fail open).
    #[must_use]
    pub fn planner_token_ceiling(&self) -> Option<i64> {
        self.limit(BROWSER_AGENT_MAX_TOKENS_PER_RUN)
            .filter(|limit| *limit > 0)
    }

    /// The org's LLM-planner switch (WS11.S2). Fails CLOSED: a read that did not happen is
    /// `Unavailable`, which the planner policy treats as "do not select the LLM planner".
    #[must_use]
    pub fn planner_flag(&self) -> PlannerFlag {
        match self {
            Self::Unavailable => PlannerFlag::Unavailable,
            Self::Rows(_) => match self.limit(BROWSER_AGENT_LLM_PLANNER) {
                Some(1) => PlannerFlag::On,
                Some(2) => PlannerFlag::Off,
                _ => PlannerFlag::Unset,
            },
        }
    }
}

/// The org's LLM-planner switch as the gateway hands it to execution-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerFlag {
    Unset,
    On,
    Off,
    Unavailable,
}

impl PlannerFlag {
    /// The value written to `tool_input.planner_flag`. `Unset` writes nothing, so a run for an
    /// org that never set the switch carries the same `tool_input` it always did.
    #[must_use]
    pub const fn wire(self) -> Option<&'static str> {
        match self {
            Self::Unset => None,
            Self::On => Some("on"),
            Self::Off => Some("off"),
            Self::Unavailable => Some("unavailable"),
        }
    }
}

/// Read ALL of an org's quota rows from org-core, with the same service credential and URL as
/// [`fetch_org_limits`]. Never fails the caller: every way the read can go wrong is
/// [`OrgQuotaKeys::Unavailable`], and each caller decides what that means.
pub async fn fetch_org_quota_keys(
    client: &reqwest::Client,
    org_core_base_url: &str,
    org_id: &str,
    service_id: &str,
    service_token: &str,
) -> OrgQuotaKeys {
    if org_core_base_url.trim().is_empty()
        || org_id.trim().is_empty()
        || service_token.trim().is_empty()
    {
        tracing::debug!(%org_id, "org-core quota read not configured; quota keys unavailable");
        return OrgQuotaKeys::Unavailable;
    }
    let url = format!(
        "{}/api/v1/organizations/{org_id}/quotas",
        org_core_base_url.trim_end_matches('/')
    );
    let request = client
        .get(&url)
        .header("x-service-id", service_id)
        .header("x-service-token", service_token);
    match request.send().await {
        Ok(response) if response.status().is_success() => match response.text().await {
            Ok(body) => OrgQuotaKeys::from_body(&body),
            Err(error) => {
                tracing::warn!(%error, %org_id, "org quota body unreadable");
                OrgQuotaKeys::Unavailable
            }
        },
        Ok(response) => {
            tracing::warn!(status = %response.status(), %org_id, "org-core rejected the quota read");
            OrgQuotaKeys::Unavailable
        }
        Err(error) => {
            tracing::warn!(%error, %org_id, "org-core unreachable for the quota read");
            OrgQuotaKeys::Unavailable
        }
    }
}

/// `min(request, org ceiling)` for the browser planner's token budget, both positive; either
/// alone when the other is absent or non-positive. A request can lower the org's ceiling and
/// never raise it, and an org ceiling that could not be read leaves the request's own value.
#[must_use]
pub fn planner_token_ceiling_for(request: Option<i64>, org_ceiling: Option<i64>) -> Option<i64> {
    let positive = |value: Option<i64>| value.filter(|value| *value > 0);
    match (positive(request), positive(org_ceiling)) {
        (Some(asked), Some(ceiling)) => Some(asked.min(ceiling)),
        (asked, ceiling) => asked.or(ceiling),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn micro_dollars_round_trip_without_truncation() {
        // The classic failure: 0.10 * 1e6 in binary floating point is
        // 99_999.999…, so truncating would store 99_999 micros — a cent short.
        assert_eq!(usd_to_micros(0.10), 100_000);
        assert_eq!(usd_to_micros(5.0), 5_000_000);
        assert!((micros_to_usd(5_000_000) - 5.0).abs() < f64::EPSILON);
        assert!((micros_to_usd(usd_to_micros(12.34)) - 12.34).abs() < 1e-9);
    }

    #[test]
    fn both_run_ceilings_are_read_from_the_payload() {
        let body = r#"{"organization_id":"org","quotas":[
            {"org_id":"org","key":"max_cost_per_run_usd_micros","value":0,"limit":5000000},
            {"org_id":"org","key":"max_tokens_per_run","value":0,"limit":1000},
            {"org_id":"org","key":"unrelated","value":0,"limit":7}
        ]}"#;
        let limits = limits_from_rows(body);
        assert!((limits.max_cost_usd.expect("cost") - 5.0).abs() < f64::EPSILON);
        assert_eq!(limits.max_tokens, Some(1000));
    }

    #[test]
    fn a_zero_or_negative_ceiling_reads_as_unset_not_as_block_everything() {
        let body = r#"{"quotas":[
            {"org_id":"o","key":"max_cost_per_run_usd_micros","value":0,"limit":0},
            {"org_id":"o","key":"max_tokens_per_run","value":0,"limit":-1}
        ]}"#;
        // Forwarding 0 would make cost-core reject every request, taking the
        // org's chat down. An unenforced cap is the lesser failure.
        assert_eq!(limits_from_rows(body), OrgQuotaLimits::default());
    }

    #[test]
    fn a_missing_absent_or_malformed_payload_yields_no_ceilings() {
        assert!(limits_from_rows(r#"{"quotas":[]}"#).is_empty());
        assert!(limits_from_rows("{}").is_empty());
        assert!(limits_from_rows("not json").is_empty());
        // An org with only unrelated quotas has no RUN ceilings.
        assert!(limits_from_rows(
            r#"{"quotas":[{"org_id":"o","key":"seats","value":0,"limit":9}]}"#
        )
        .is_empty());
    }

    #[test]
    fn the_cost_rows_reset_period_governs_the_window_else_the_token_rows() {
        let both = r#"{"quotas":[
            {"org_id":"o","key":"max_cost_per_run_usd_micros","value":0,"limit":5000000,"reset_period":"monthly"},
            {"org_id":"o","key":"max_tokens_per_run","value":0,"limit":1000,"reset_period":"daily"}
        ]}"#;
        assert_eq!(limits_from_rows(both).reset_period, ResetPeriod::Monthly);

        // A shared window must not shorten either cap: the widest period wins,
        // so the monthly token ceiling is never counted from the start of a day.
        let cost_daily_tokens_monthly = r#"{"quotas":[
            {"org_id":"o","key":"max_cost_per_run_usd_micros","value":0,"limit":5000000,"reset_period":"daily"},
            {"org_id":"o","key":"max_tokens_per_run","value":0,"limit":1000,"reset_period":"monthly"}
        ]}"#;
        assert_eq!(
            limits_from_rows(cost_daily_tokens_monthly).reset_period,
            ResetPeriod::Monthly
        );

        let cost_none_tokens_daily = r#"{"quotas":[
            {"org_id":"o","key":"max_cost_per_run_usd_micros","value":0,"limit":5000000,"reset_period":"none"},
            {"org_id":"o","key":"max_tokens_per_run","value":0,"limit":1000,"reset_period":"daily"}
        ]}"#;
        assert_eq!(
            limits_from_rows(cost_none_tokens_daily).reset_period,
            ResetPeriod::None
        );

        let tokens_only = r#"{"quotas":[
            {"org_id":"o","key":"max_tokens_per_run","value":0,"limit":1000,"reset_period":"daily"}
        ]}"#;
        assert_eq!(
            limits_from_rows(tokens_only).reset_period,
            ResetPeriod::Daily
        );
        assert_eq!(limits_from_rows("{}").reset_period, ResetPeriod::None);
    }

    #[test]
    fn a_none_period_has_no_window_and_monthly_starts_on_the_first() {
        let now = Utc.with_ymd_and_hms(2026, 10, 23, 15, 42, 10).unwrap();
        assert_eq!(window_since(ResetPeriod::None, now), None);
        assert_eq!(
            window_since(ResetPeriod::Monthly, now),
            Some(Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap())
        );
    }

    // ---- WS11.S1 / S2: the browser planner's quota keys ----

    fn rows(body: &str) -> OrgQuotaKeys {
        OrgQuotaKeys::from_body(body)
    }

    #[test]
    fn planner_token_ceiling_is_min_of_request_and_org_key() {
        let org = rows(
            r#"{"quotas":[{"org_id":"o","key":"browser_agent_max_tokens_per_run","value":0,"limit":50000}]}"#,
        );
        assert_eq!(org.planner_token_ceiling(), Some(50_000));
        // A request below the ceiling lowers it; the org's key alone binds an unasking request.
        assert_eq!(planner_token_ceiling_for(Some(20_000), org.planner_token_ceiling()), Some(20_000));
        assert_eq!(planner_token_ceiling_for(None, org.planner_token_ceiling()), Some(50_000));
        // The request alone, with no org key, passes through for execution-core's own ceilings.
        assert_eq!(planner_token_ceiling_for(Some(20_000), None), Some(20_000));
        assert_eq!(planner_token_ceiling_for(None, None), None);
        // A non-positive value on either side is no bound.
        assert_eq!(planner_token_ceiling_for(Some(0), Some(-5)), None);
        assert_eq!(planner_token_ceiling_for(Some(-1), Some(50_000)), Some(50_000));
        // The chat run's own token key is a different key and is never read here.
        let chat_only = rows(
            r#"{"quotas":[{"org_id":"o","key":"max_tokens_per_run","value":0,"limit":999}]}"#,
        );
        assert_eq!(chat_only.planner_token_ceiling(), None);
    }

    #[test]
    fn request_cannot_raise_the_org_ceiling() {
        assert_eq!(planner_token_ceiling_for(Some(900_000), Some(50_000)), Some(50_000));
        assert_eq!(planner_token_ceiling_for(Some(50_001), Some(50_000)), Some(50_000));
        assert_eq!(planner_token_ceiling_for(Some(i64::MAX), Some(1)), Some(1));
    }

    #[tokio::test]
    async fn unreadable_org_quota_leaves_the_default_in_place() {
        // Unconfigured, unreachable and garbled all read as Unavailable, and an unavailable read
        // carries no ceiling: the request's value (or nothing) stays, execution-core's own
        // default and ceiling still apply.
        let client = reqwest::Client::new();
        assert_eq!(
            fetch_org_quota_keys(&client, "", "org", "svc", "tok").await,
            OrgQuotaKeys::Unavailable
        );
        assert_eq!(
            fetch_org_quota_keys(&client, "http://127.0.0.1:1", "org", "svc", "").await,
            OrgQuotaKeys::Unavailable
        );
        let unreachable = fetch_org_quota_keys(&client, "http://127.0.0.1:1", "org", "svc", "tok").await;
        assert_eq!(unreachable, OrgQuotaKeys::Unavailable);
        assert_eq!(unreachable.planner_token_ceiling(), None);
        assert_eq!(planner_token_ceiling_for(None, unreachable.planner_token_ceiling()), None);
        assert_eq!(
            planner_token_ceiling_for(Some(20_000), unreachable.planner_token_ceiling()),
            Some(20_000)
        );
        assert_eq!(rows("not json"), OrgQuotaKeys::Unavailable);
        // An org that simply has no rows is a decision, not an outage.
        assert_eq!(rows(r#"{"quotas":[]}"#), OrgQuotaKeys::Rows(Vec::new()));
    }

    #[tokio::test]
    async fn quota_keys_are_read_over_http_with_the_service_credential() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/organizations/org-a/quotas"))
            .and(header("x-service-id", "model-gateway"))
            .and(header("x-service-token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"quotas":[
                    {"org_id":"org-a","key":"browser_agent_max_tokens_per_run","value":0,"limit":40000},
                    {"org_id":"org-a","key":"browser_agent_llm_planner","value":0,"limit":1}
                ]}"#,
            ))
            .mount(&server)
            .await;
        let keys = fetch_org_quota_keys(&reqwest::Client::new(), &server.uri(), "org-a", "model-gateway", "tok").await;
        assert_eq!(keys.planner_token_ceiling(), Some(40_000));
        assert_eq!(keys.planner_flag(), PlannerFlag::On);

        // A refusal is Unavailable, never an empty org.
        let refusing = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&refusing)
            .await;
        let keys = fetch_org_quota_keys(&reqwest::Client::new(), &refusing.uri(), "org-a", "model-gateway", "tok").await;
        assert_eq!(keys, OrgQuotaKeys::Unavailable);
    }

    #[test]
    fn planner_flag_one_is_on_two_is_off() {
        let flag = |limit: i64| {
            rows(&format!(
                r#"{{"quotas":[{{"org_id":"o","key":"browser_agent_llm_planner","value":0,"limit":{limit}}}]}}"#
            ))
            .planner_flag()
        };
        assert_eq!(flag(1), PlannerFlag::On);
        assert_eq!(flag(2), PlannerFlag::Off);
        // Any other value, including 0 and negatives, is unset, not a third state.
        for other in [0, 3, 7, -1, 100] {
            assert_eq!(flag(other), PlannerFlag::Unset, "{other}");
        }
        // No row at all is unset, and unset writes nothing to tool_input.
        assert_eq!(rows(r#"{"quotas":[]}"#).planner_flag(), PlannerFlag::Unset);
        assert_eq!(PlannerFlag::Unset.wire(), None);
        assert_eq!(PlannerFlag::On.wire(), Some("on"));
        assert_eq!(PlannerFlag::Off.wire(), Some("off"));
        assert_eq!(PlannerFlag::Unavailable.wire(), Some("unavailable"));
    }

    #[tokio::test]
    async fn planner_flag_read_error_is_unavailable() {
        // Fail CLOSED: where the token ceiling treats an unreadable read as "no ceiling", the
        // planner switch treats it as "unavailable", which keeps the LLM planner off.
        let client = reqwest::Client::new();
        for keys in [
            fetch_org_quota_keys(&client, "http://127.0.0.1:1", "org", "svc", "tok").await,
            fetch_org_quota_keys(&client, "", "org", "svc", "tok").await,
            rows("<html>502</html>"),
        ] {
            assert_eq!(keys.planner_flag(), PlannerFlag::Unavailable);
        }
    }
}
