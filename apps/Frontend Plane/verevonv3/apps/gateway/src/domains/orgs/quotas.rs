use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser, upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

/// Quota keys the Model Plane actually enforces.
///
/// These mirror the constants in model-gateway's `org_quota.rs`
/// (`MAX_COST_PER_RUN_USD_MICROS`, `MAX_TOKENS_PER_RUN`), which is the only
/// consumer of org-core's quota rows: it reads them and hands the ceiling to
/// cost-core's budget check.
///
/// The allowlist is deliberate rather than a passthrough of whatever key a
/// client sends. org-core will happily store an arbitrary key, and a key
/// nothing reads is worse than no key at all — it tells an operator they have
/// capped spend when nothing is enforcing it. Adding a key here should happen
/// in the same change that teaches something to read it.
const ENFORCED_QUOTA_KEYS: [&str; 2] = ["max_cost_per_run_usd_micros", "max_tokens_per_run"];

/// Reset periods org-core accepts (`org.Quota.ResetPeriod`).
const VALID_RESET_PERIODS: [&str; 3] = ["daily", "monthly", "none"];

/// GET /api/v1/orgs/:id/quotas — an org's configured spend/token ceilings.
///
/// Control Plane owns quotas (CLAUDE.md), so this proxies to org-core rather
/// than deriving anything locally. Org-admin gated: a quota reveals the
/// organization's commercial posture and is settable on the same surface.
pub(super) async fn list_quotas(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let url = format!(
        "{}/api/v1/organizations/{}/quotas",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/quotas/:key — set one quota's LIMIT.
///
/// Only the ceiling is settable. Accumulated usage (`quota_value`) belongs to
/// whatever meters it, so raising a limit never hands back a fresh allowance —
/// that asymmetry is org-core's and this route must not paper over it.
///
/// `limit` is validated as a non-negative integer here rather than forwarded
/// blindly: 0 is a legitimate ceiling ("no allowance") and must stay
/// distinguishable from an omitted field, while a negative ceiling is
/// meaningless and would otherwise be stored as an unreachable cap.
pub(crate) async fn set_quota(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, key)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }

    if !ENFORCED_QUOTA_KEYS.contains(&key.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                &format!(
                    "unknown quota key '{key}'. Settable keys: {}.",
                    ENFORCED_QUOTA_KEYS.join(", ")
                ),
            )),
        );
    }

    // Absent and 0 must not collapse: org-core itself takes `limit` as a
    // pointer for exactly this reason.
    let Some(limit) = body.get("limit").and_then(Value::as_i64) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "limit is required and must be an integer.",
            )),
        );
    };
    if limit < 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "limit must be zero or greater. Zero means no allowance.",
            )),
        );
    }

    let reset_period = body
        .get("reset_period")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(period) = reset_period {
        if !VALID_RESET_PERIODS.contains(&period) {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "validation_error",
                    &format!(
                        "reset_period must be one of: {}.",
                        VALID_RESET_PERIODS.join(", ")
                    ),
                )),
            );
        }
    }

    let url = format!(
        "{}/api/v1/organizations/{}/quotas/{}",
        state.org_core_url,
        urlencoding::encode(&id),
        urlencoding::encode(&key)
    );
    let mut payload = json!({ "limit": limit });
    if let Some(period) = reset_period {
        payload["reset_period"] = json!(period);
    }
    proxy_json(
        &state,
        Method::PUT,
        &url,
        Some(payload),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enforced_quota_keys_are_settable() {
        // Every settable key must have a reader in model-gateway's org_quota.rs.
        // If this list grows without a corresponding reader, an operator can set
        // a cap that nothing enforces.
        assert!(ENFORCED_QUOTA_KEYS.contains(&"max_cost_per_run_usd_micros"));
        assert!(ENFORCED_QUOTA_KEYS.contains(&"max_tokens_per_run"));
        assert_eq!(ENFORCED_QUOTA_KEYS.len(), 2);
    }

    #[test]
    fn unknown_keys_are_not_silently_accepted() {
        for key in ["seats", "api_calls", "storage_gb", "", "MAX_TOKENS_PER_RUN"] {
            assert!(
                !ENFORCED_QUOTA_KEYS.contains(&key),
                "{key} must not be settable without a reader"
            );
        }
    }

    #[test]
    fn reset_periods_match_org_core() {
        // org.Quota.ResetPeriod's documented values.
        assert_eq!(VALID_RESET_PERIODS, ["daily", "monthly", "none"]);
    }
}
