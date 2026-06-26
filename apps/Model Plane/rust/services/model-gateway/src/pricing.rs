//! Model pricing cache — turns reported token counts into a USD cost for the
//! streamed `Usage` SSE event, using cost-core's price catalogue as the single
//! source of truth.
//!
//! cost-core prices the durable ledger authoritatively (so the dollar ledger
//! and budget posture are real); this cache lets the SSE show the *same* figure
//! without a per-request hop to the database. It fetches `GET /api/v1/pricing`
//! once and refreshes lazily once the TTL lapses. If cost-core is unreachable
//! and the cache is cold, [`PricingCache::cost_usd`] returns `None` — the
//! gateway emits a null cost rather than a fabricated one.
//!
//! Matching mirrors cost-core's resolver: exact key, else the longest model key
//! that is a prefix of the reported name, else the mandatory `default` row.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

const DEFAULT_MODEL_KEY: &str = "default";
const REFRESH_TTL: Duration = Duration::from_secs(600);
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize)]
struct Rate {
    model: String,
    #[serde(default)]
    input_per_million: f64,
    #[serde(default)]
    output_per_million: f64,
}

#[derive(Deserialize)]
struct PricingResponse {
    #[serde(default)]
    rates: Vec<Rate>,
}

#[derive(Default)]
struct CacheInner {
    /// Catalogue keyed by a normalized (lowercased, trimmed) model key.
    rates: Vec<Rate>,
    fetched_at: Option<Instant>,
}

/// Shared, cheaply-cloneable pricing cache (Arc-backed).
#[derive(Clone)]
pub struct PricingCache {
    /// cost-core base URL (e.g. `http://model-plane-cost-core-1:8089`). `None`
    /// disables pricing — `cost_usd` then always returns `None`.
    base_url: Option<String>,
    http: reqwest::Client,
    inner: Arc<RwLock<CacheInner>>,
}

impl PricingCache {
    /// Build a cache against cost-core's base URL. An empty/blank URL disables
    /// pricing (the gateway then emits a null cost — never a fake one).
    #[must_use]
    pub fn new(base_url: Option<String>, http: reqwest::Client) -> Self {
        let base_url = base_url
            .map(|u| u.trim().trim_end_matches('/').to_owned())
            .filter(|u| !u.is_empty());
        Self {
            base_url,
            http,
            inner: Arc::new(RwLock::new(CacheInner::default())),
        }
    }

    /// Compute the USD cost for an inference. Returns `None` when no catalogue
    /// is available (pricing disabled, or cost-core unreachable with a cold
    /// cache) so the caller can emit a null `cost_usd` instead of faking one.
    pub async fn cost_usd(&self, model: &str, input_tokens: i64, output_tokens: i64) -> Option<f64> {
        self.ensure_fresh().await;
        let inner = self.inner.read().await;
        if inner.rates.is_empty() {
            return None;
        }
        let rate = lookup(&inner.rates, model)?;
        // Token counts comfortably fit in u32; the u32→f64 conversion is
        // lossless (avoids clippy::cast_precision_loss on a raw i64 cast).
        let input = f64::from(u32::try_from(input_tokens.max(0)).unwrap_or(u32::MAX));
        let output = f64::from(u32::try_from(output_tokens.max(0)).unwrap_or(u32::MAX));
        Some(input / 1_000_000.0 * rate.input_per_million + output / 1_000_000.0 * rate.output_per_million)
    }

    /// Refresh the catalogue from cost-core when it is empty or past the TTL. A
    /// fetch failure leaves the cache as-is (and `fetched_at` unset on a cold
    /// cache, so the next call retries) — never poisons it with bad data.
    async fn ensure_fresh(&self) {
        let Some(base) = self.base_url.as_ref() else {
            return;
        };
        {
            let inner = self.inner.read().await;
            if !inner.rates.is_empty() {
                if let Some(at) = inner.fetched_at {
                    if at.elapsed() < REFRESH_TTL {
                        return;
                    }
                }
            }
        }
        let url = format!("{base}/api/v1/pricing");
        match self.http.get(&url).timeout(FETCH_TIMEOUT).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<PricingResponse>().await {
                Ok(parsed) => {
                    let mut rates = parsed.rates;
                    for rate in &mut rates {
                        rate.model = rate.model.trim().to_lowercase();
                    }
                    let mut inner = self.inner.write().await;
                    inner.rates = rates;
                    inner.fetched_at = Some(Instant::now());
                }
                Err(error) => tracing::warn!(%error, "pricing: failed to parse catalogue"),
            },
            Ok(resp) => tracing::warn!(status = %resp.status(), "pricing: catalogue fetch non-2xx"),
            Err(error) => {
                tracing::warn!(%error, "pricing: cost-core unreachable; cost stays null until reachable");
            }
        }
    }
}

/// Resolve a model name to a rate: exact key, else the longest prefix key, else
/// the `default` row.
fn lookup<'a>(rates: &'a [Rate], model: &str) -> Option<&'a Rate> {
    let key = model.trim().to_lowercase();
    if !key.is_empty() {
        if let Some(rate) = rates.iter().find(|r| r.model == key) {
            return Some(rate);
        }
        let mut best: Option<&Rate> = None;
        let mut best_len = 0usize;
        for rate in rates {
            if rate.model == DEFAULT_MODEL_KEY {
                continue;
            }
            if key.starts_with(&rate.model) && rate.model.len() > best_len {
                best = Some(rate);
                best_len = rate.model.len();
            }
        }
        if best.is_some() {
            return best;
        }
    }
    rates.iter().find(|r| r.model == DEFAULT_MODEL_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates() -> Vec<Rate> {
        vec![
            Rate { model: "default".into(), input_per_million: 3.0, output_per_million: 15.0 },
            Rate { model: "gpt-4o-mini".into(), input_per_million: 0.15, output_per_million: 0.60 },
            Rate { model: "claude-sonnet".into(), input_per_million: 3.0, output_per_million: 15.0 },
        ]
    }

    #[test]
    fn exact_and_prefix_and_default() {
        let r = rates();
        assert_eq!(lookup(&r, "gpt-4o-mini").unwrap().model, "gpt-4o-mini");
        assert_eq!(lookup(&r, "claude-sonnet-4-6").unwrap().model, "claude-sonnet");
        assert_eq!(lookup(&r, "totally-unknown").unwrap().model, "default");
    }

    #[tokio::test]
    async fn disabled_cache_returns_none() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        assert!(cache.cost_usd("gpt-4o-mini", 1000, 500).await.is_none());
    }
}
