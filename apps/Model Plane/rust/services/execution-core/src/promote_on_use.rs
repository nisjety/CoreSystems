//! Phase 8 — promote-on-use. The promote PRIMITIVE already exists (quarry
//! `/v1/scrape|/v1/crawl|/v1/batch` with `ingest:true`); this is the TRIGGER:
//! count grounded `web_fetch` uses per (org, user, url) and, past a threshold,
//! promote the live page into the durable knowledge base so the agent stops
//! re-fetching content it keeps relying on.
//!
//! SAFE BY DEFAULT — two-key opt-in, dry-run-first (mirrors the Phase 7
//! retention janitor). With no config it is a complete no-op:
//!   - `PROMOTE_ON_USE_ENABLED=true`  → start counting + LOG promote candidates
//!     (dry-run; performs no ingest).
//!   - `PROMOTE_ON_USE_EXECUTE=true`  → additionally perform the real ingest.
//!   - `PROMOTE_ON_USE_THRESHOLD`     → grounded-use count to fire at (default 3).
//!
//! This is a read-path side effect (the plan's flagged-riskiest element), so it
//! defaults OFF and, when enabled, defaults to dry-run. Two REFINEMENTS are
//! required before arming EXECUTE in production:
//!   1. Ownership — the Edge `/v1/scrape` derives owner from the JWT
//!      `claims.user_id`; execution-core holds only a service token, so today an
//!      executed promote would attribute the doc to the service principal, not
//!      the agent's user. A per-user (data-plane/quarry) token must be minted so
//!      the promoted doc lands owner=user, visibility=private.
//!   2. Signal — "grounded use" here is a successful `web_fetch`; a truer signal
//!      is the page actually being CITED in the model's answer. The counter is
//!      also process-local (in-memory); a durable per-org store survives
//!      restarts.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct Config {
    enabled: bool,
    execute: bool,
    threshold: u32,
}

fn config() -> &'static Config {
    static CFG: OnceLock<Config> = OnceLock::new();
    CFG.get_or_init(|| Config {
        enabled: std::env::var("PROMOTE_ON_USE_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false),
        execute: std::env::var("PROMOTE_ON_USE_EXECUTE")
            .map(|v| v == "true")
            .unwrap_or(false),
        threshold: std::env::var("PROMOTE_ON_USE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(3),
    })
}

fn counts() -> &'static Mutex<HashMap<String, u32>> {
    static COUNTS: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a grounded use of `url` by (org, user). No-op unless enabled. When
/// the per-(org,user,url) count reaches the threshold, promotes exactly once
/// (dry-run logs unless `PROMOTE_ON_USE_EXECUTE=true`). Fire-and-forget — never
/// affects the calling tool's result.
pub async fn maybe_promote(org_id: &str, user_id: &str, url: &str) {
    let cfg = config();
    if !cfg.enabled || org_id.is_empty() || user_id.is_empty() || url.is_empty() {
        return;
    }
    // Separator can't appear in the components; keeps the composite key unambiguous.
    let key = format!("{org_id}\u{1}{user_id}\u{1}{url}");
    let fire = match counts().lock() {
        Ok(mut map) => {
            let n = map.entry(key).or_insert(0);
            *n += 1;
            *n == cfg.threshold // fire EXACTLY once, at the threshold
        }
        Err(_) => return,
    };
    if !fire {
        return;
    }
    if !cfg.execute {
        tracing::info!(
            url,
            %user_id,
            threshold = cfg.threshold,
            "promote-on-use: candidate reached threshold (DRY-RUN — not promoted; set PROMOTE_ON_USE_EXECUTE=true to arm)"
        );
        return;
    }
    match promote(org_id, user_id, url).await {
        Ok(()) => tracing::info!(url, %user_id, "promote-on-use: promoted page to knowledge base"),
        Err(e) => tracing::warn!(url, error = %e, "promote-on-use: promote failed"),
    }
}

/// Perform the promote via Quarry's ingest primitive with a tenant-bound
/// execution-core service principal. `x-user-id` is advisory attribution only;
/// Quarry derives authority and tenant from the signed bearer.
async fn promote(org_id: &str, user_id: &str, url: &str) -> Result<(), String> {
    let base = std::env::var("QUARRY_EDGE_URL")
        .or_else(|_| std::env::var("QUARRY_EDGE_ADDR"))
        .map_err(|_| "QUARRY_EDGE_URL/ADDR unset".to_string())?;
    let endpoint = format!("{}/v1/scrape", base.trim_end_matches('/'));
    let http = reqwest::Client::new();
    let auth = crate::quarry_auth::TokenSource::from_env()
        .map_err(|error| format!("quarry authentication failed: {error}"))?;
    let body = serde_json::json!({ "url": url, "org_id": org_id, "ingest": true });
    let mut retried_unauthorized = false;
    let resp = loop {
        let token = auth
            .token(org_id, &["scrape:write"])
            .await
            .map_err(|error| format!("quarry authentication failed: {error}"))?;
        let resp = http
            .post(&endpoint)
            .bearer_auth(&token)
            .header("x-quarry-org", org_id)
            .header("x-user-id", user_id)
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
            auth.invalidate_if_matches(org_id, &["scrape:write"], &token)
                .await
                .map_err(|error| format!("quarry authentication failed: {error}"))?;
            retried_unauthorized = true;
            continue;
        }
        break resp;
    };
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("status {}", resp.status()))
    }
}
