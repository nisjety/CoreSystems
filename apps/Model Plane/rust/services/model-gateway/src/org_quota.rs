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

/// The ceilings an org has configured. `None` means "no ceiling set", which is
/// different from a ceiling of zero — zero would forbid everything.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OrgQuotaLimits {
    pub max_cost_usd: Option<f64>,
    pub max_tokens: Option<i64>,
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
            .map(|row| row.limit)
            .filter(|limit| *limit > 0)
    };
    OrgQuotaLimits {
        max_cost_usd: find(MAX_COST_PER_RUN_USD_MICROS).map(micros_to_usd),
        max_tokens: find(MAX_TOKENS_PER_RUN),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
