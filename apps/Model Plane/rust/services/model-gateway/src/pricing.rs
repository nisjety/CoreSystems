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
//!
//! Only metered calls are priced. A call served by the user's own `ChatGPT`
//! subscription ([`CostBasis::Subscription`]) is never priced from API list
//! prices, whatever tokens it reports.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

const DEFAULT_MODEL_KEY: &str = "default";
const REFRESH_TTL: Duration = Duration::from_secs(600);
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);

/// A prompt-cache read is billed at this fraction of the base input-token
/// rate -- the documented 10x discount Anthropic and the GPT-5 family apply
/// to a cache hit. The rate for every model [`cache_rates`] does not list.
const CACHE_READ_DISCOUNT: f64 = 0.1;

/// Provider ids that serve a call through the user's own `ChatGPT`
/// subscription (Integration Core's Codex broker): the registry id and the
/// aliases `normalize` accepts for it.
const SUBSCRIPTION_PROVIDERS: [&str; 3] = [
    "openai-codex-subscription",
    "codex-subscription",
    "chatgpt-codex",
];

/// How a call's `cost_usd` was established. It travels beside the cost on
/// the `usage` SSE event and the `USAGE_ENVELOPE`, so a reader can tell a
/// call that cost nothing per token from a free one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostBasis {
    /// Priced per token against cost-core's catalogue: an API route.
    Metered,
    /// Served by the user's own `ChatGPT` subscription. There is no per-token
    /// charge to price, so the cost is 0.0 and the token counts are usage,
    /// not spend. API list prices never apply.
    Subscription,
}

impl CostBasis {
    /// The basis a call's route implies: subscription when it carries a
    /// subscription connection or asks for the subscription provider.
    #[must_use]
    pub fn of_route(provider_hint: &str, subscription_connection_id: &str) -> Self {
        if !subscription_connection_id.trim().is_empty() || is_subscription_provider(provider_hint)
        {
            Self::Subscription
        } else {
            Self::Metered
        }
    }

    /// The basis once the serving provider is known. The provider that
    /// actually served decides; an empty provenance, from a completion that
    /// made no provider call of its own (a checked artifact's synthetic
    /// answer), keeps the route's basis.
    #[must_use]
    pub fn served_by(self, provider_used: &str) -> Self {
        if provider_used.trim().is_empty() {
            self
        } else if is_subscription_provider(provider_used) {
            Self::Subscription
        } else {
            Self::Metered
        }
    }

    /// The wire value of the `cost_basis` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metered => "metered",
            Self::Subscription => "subscription",
        }
    }
}

fn is_subscription_provider(provider: &str) -> bool {
    let provider = provider.trim();
    SUBSCRIPTION_PROVIDERS
        .iter()
        .any(|id| provider.eq_ignore_ascii_case(id))
}

/// Cache-leg prices for one model, as multiples of its base input rate. Both
/// legs are already counted inside `input_tokens` at the full rate; these say
/// what each leg should have cost instead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CacheRates {
    read: f64,
    write: f64,
}

/// The cache-leg prices for a reported model name.
///
/// Only rates `OpenAI` documents are listed: gpt-4o-mini's cached input is half
/// its input rate, the GPT-5 family's is a tenth, and GPT-5.6 and later also
/// bill a cache write at 1.25x (earlier models do not charge for writes).
/// Every other model keeps what this module applied before per-model rates
/// existed: [`CACHE_READ_DISCOUNT`] for reads and the plain input rate for
/// writes. That includes gpt-4o, gpt-4.1, the o-series, `model-router` and
/// Claude, whose rates are not confirmed here.
///
/// Keyed on the model name the provider served, not on the catalogue row it
/// priced against: the GPT-5 models have no row of their own and resolve to
/// `default`.
///
/// cost-core's Go resolver (`services/cost-core/internal/pricing`,
/// `cacheRatesFor`) mirrors this table, so the streamed display cost and the
/// ledgered cost agree. Keep the two in step.
fn cache_rates(model: &str) -> CacheRates {
    let model = model.trim().to_ascii_lowercase();
    // Also matches dated snapshots such as `gpt-4o-mini-2024-07-18`.
    if model.starts_with("gpt-4o-mini") {
        return CacheRates {
            read: 0.5,
            write: 1.0,
        };
    }
    if let Some(rest) = model.strip_prefix("gpt-5") {
        let minor: String = rest
            .strip_prefix('.')
            .unwrap_or("")
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let bills_writes = minor.parse::<u32>().is_ok_and(|minor| minor >= 6);
        return CacheRates {
            read: 0.1,
            write: if bills_writes { 1.25 } else { 1.0 },
        };
    }
    CacheRates {
        read: CACHE_READ_DISCOUNT,
        write: 1.0,
    }
}

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
    ///
    /// `cache_read_input_tokens` and `cache_creation_input_tokens` are the
    /// same cache legs already folded into `input_tokens` (see
    /// `inference-core`'s `total_input_tokens` and `openai_cache_tokens`):
    /// pass 0 for a request that reported no cache usage, which reproduces
    /// the pre-cache-pricing result exactly. Each leg is re-priced at the
    /// model's [`cache_rates`]: a read below the input rate, and a write at
    /// the input rate unless the model bills cache writes (GPT-5.6 and later).
    pub async fn cost_usd(
        &self,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
    ) -> Option<f64> {
        self.ensure_fresh().await;
        let inner = self.inner.read().await;
        if inner.rates.is_empty() {
            return None;
        }
        let rate = lookup(&inner.rates, model)?;
        Some(cost_from_rate(
            rate,
            cache_rates(model),
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        ))
    }

    /// [`Self::cost_usd`] for a call made on `basis`. A subscription call is
    /// never priced from API list prices: its cost is a known `Some(0.0)`,
    /// not the unavailable `None`, whatever tokens it reports.
    pub async fn cost_usd_for(
        &self,
        basis: CostBasis,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
    ) -> Option<f64> {
        match basis {
            CostBasis::Subscription => Some(0.0),
            CostBasis::Metered => {
                self.cost_usd(
                    model,
                    input_tokens,
                    output_tokens,
                    cache_read_input_tokens,
                    cache_creation_input_tokens,
                )
                .await
            }
        }
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

/// Price a token breakdown against a resolved rate. Pure and synchronous so
/// the cache-tier discount formula is unit-testable without the async
/// HTTP-backed catalogue (`PricingCache::cost_usd` is the async wrapper that
/// resolves `rate` via [`lookup`] and calls this).
///
/// Both cache legs are already folded into `input_tokens` (see
/// `inference-core`'s `total_input_tokens`) — passing 0 for both reproduces
/// the pre-cache-pricing result exactly, byte for byte. `input` is charged in
/// full first; then the cache-read leg's over-charge is credited back down to
/// `cache.read`, and the cache-write leg is surcharged up to `cache.write`
/// (a no-op at the default 1.0, which adds exactly 0.0).
fn cost_from_rate(
    rate: &Rate,
    cache: CacheRates,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
) -> f64 {
    // Token counts comfortably fit in u32; the u32→f64 conversion is
    // lossless (avoids clippy::cast_precision_loss on a raw i64 cast).
    let input = f64::from(u32::try_from(input_tokens.max(0)).unwrap_or(u32::MAX));
    let output = f64::from(u32::try_from(output_tokens.max(0)).unwrap_or(u32::MAX));
    let cache_read = f64::from(u32::try_from(cache_read_input_tokens.max(0)).unwrap_or(u32::MAX));
    let cache_write =
        f64::from(u32::try_from(cache_creation_input_tokens.max(0)).unwrap_or(u32::MAX));
    input / 1_000_000.0 * rate.input_per_million
        + output / 1_000_000.0 * rate.output_per_million
        - cache_read / 1_000_000.0 * rate.input_per_million * (1.0 - cache.read)
        + cache_write / 1_000_000.0 * rate.input_per_million * (cache.write - 1.0)
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
            Rate {
                model: "default".into(),
                input_per_million: 3.0,
                output_per_million: 15.0,
            },
            Rate {
                model: "gpt-4o-mini".into(),
                input_per_million: 0.15,
                output_per_million: 0.60,
            },
            Rate {
                model: "claude-sonnet".into(),
                input_per_million: 3.0,
                output_per_million: 15.0,
            },
        ]
    }

    #[test]
    fn cache_read_tokens_zero_reproduces_the_pre_cache_pricing_result() {
        // Purely additive: a caller that passes 0 cache tokens (every caller
        // before this change, and every non-Anthropic provider today) must
        // get byte-for-byte the same cost as before the cache-tier discount
        // existed.
        let rate = Rate {
            model: "claude-sonnet".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
        };
        let plain = 1_000.0 / 1_000_000.0 * 3.0 + 500.0 / 1_000_000.0 * 15.0;
        for model in ["claude-sonnet-4-6", "gpt-4o-mini", "gpt-5.6-terra"] {
            let got = cost_from_rate(&rate, cache_rates(model), 1_000, 500, 0, 0);
            assert!((got - plain).abs() < 1e-12, "{model}");
        }
    }

    #[test]
    fn cache_read_tokens_are_billed_at_a_tenth_of_the_input_rate() {
        // 8_520 input_tokens folds in an 8_000-token cache read (Anthropic's
        // `total_input_tokens` fold-in). Only 520 tokens are fresh input;
        // the other 8_000 are billed at CACHE_READ_DISCOUNT (0.1x) of the
        // $3/1M input rate, not the full rate.
        let rate = Rate {
            model: "claude-sonnet".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
        };
        let got = cost_from_rate(&rate, cache_rates("claude-sonnet-4-6"), 8_520, 42, 8_000, 0);
        let want = 520.0 / 1_000_000.0 * 3.0 // fresh input, full rate
            + 8_000.0 / 1_000_000.0 * 3.0 * 0.1 // cached input, 10x discount
            + 42.0 / 1_000_000.0 * 15.0; // output, unaffected
        assert!(
            (got - want).abs() < 1e-12,
            "got {got}, want {want} (a naive full-rate charge on all 8,520 input tokens would be {})",
            8_520.0 / 1_000_000.0 * 3.0 + 42.0 / 1_000_000.0 * 15.0
        );
    }

    #[test]
    fn cache_rates_list_only_the_documented_openai_models() {
        let default = CacheRates {
            read: CACHE_READ_DISCOUNT,
            write: 1.0,
        };
        let half = CacheRates {
            read: 0.5,
            write: 1.0,
        };
        let gpt5 = CacheRates {
            read: 0.1,
            write: 1.0,
        };
        let gpt56 = CacheRates {
            read: 0.1,
            write: 1.25,
        };
        for (model, want) in [
            ("gpt-4o-mini", half),
            ("gpt-4o-mini-2024-07-18", half),
            (" GPT-4o-Mini ", half),
            ("gpt-5", gpt5),
            ("gpt-5-mini", gpt5),
            ("gpt-5-nano", gpt5),
            ("gpt-5.2", gpt5),
            ("gpt-5.5", gpt5),
            ("gpt-5.6", gpt56),
            ("gpt-5.6-terra", gpt56),
            ("gpt-5.10", gpt56),
            // Not confirmed here, so unchanged from the flat rate.
            ("gpt-4o", default),
            ("gpt-4.1-mini", default),
            ("o4-mini", default),
            ("model-router", default),
            ("claude-sonnet-4-6", default),
            ("", default),
        ] {
            assert_eq!(cache_rates(model), want, "{model:?}");
        }
    }

    #[test]
    fn gpt_4o_mini_cache_reads_are_billed_at_half_the_input_rate() {
        let rate = Rate {
            model: "gpt-4o-mini".into(),
            input_per_million: 0.15,
            output_per_million: 0.60,
        };
        let got = cost_from_rate(
            &rate,
            cache_rates("gpt-4o-mini-2024-07-18"),
            1_566,
            2,
            1_408,
            0,
        );
        let want = 158.0 / 1_000_000.0 * 0.15 // fresh input
            + 1_408.0 / 1_000_000.0 * 0.15 * 0.5 // cached input, half rate
            + 2.0 / 1_000_000.0 * 0.60;
        assert!((got - want).abs() < 1e-12, "got {got}, want {want}");
    }

    #[test]
    fn gpt_5_6_cache_writes_are_surcharged_to_one_and_a_quarter() {
        let rate = Rate {
            model: "default".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
        };
        // 2,000 prompt tokens: 1,024 read from the cache, 512 written to it,
        // 464 plain input.
        let got = cost_from_rate(&rate, cache_rates("gpt-5.6"), 2_000, 50, 1_024, 512);
        let want = 464.0 / 1_000_000.0 * 3.0
            + 1_024.0 / 1_000_000.0 * 3.0 * 0.1
            + 512.0 / 1_000_000.0 * 3.0 * 1.25
            + 50.0 / 1_000_000.0 * 15.0;
        assert!((got - want).abs() < 1e-12, "got {got}, want {want}");
    }

    #[test]
    fn a_write_leg_is_plain_input_for_models_that_do_not_bill_writes() {
        let rate = Rate {
            model: "claude-sonnet".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
        };
        let with_write = cost_from_rate(&rate, cache_rates("claude-sonnet-4-6"), 2_000, 50, 0, 512);
        let without = cost_from_rate(&rate, cache_rates("claude-sonnet-4-6"), 2_000, 50, 0, 0);
        assert!(
            (with_write - without).abs() < 1e-12,
            "unchanged from before per-model rates"
        );
    }

    #[test]
    fn exact_and_prefix_and_default() {
        let r = rates();
        assert_eq!(lookup(&r, "gpt-4o-mini").unwrap().model, "gpt-4o-mini");
        assert_eq!(
            lookup(&r, "claude-sonnet-4-6").unwrap().model,
            "claude-sonnet"
        );
        assert_eq!(lookup(&r, "totally-unknown").unwrap().model, "default");
    }

    #[tokio::test]
    async fn disabled_cache_returns_none() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        assert!(cache
            .cost_usd("gpt-4o-mini", 1000, 500, 0, 0)
            .await
            .is_none());
    }

    #[test]
    fn cost_basis_follows_the_serving_provider_and_falls_back_to_the_route() {
        use CostBasis::{Metered, Subscription};
        assert_eq!(
            CostBasis::of_route("openai-codex-subscription", "conn-1"),
            Subscription
        );
        assert_eq!(CostBasis::of_route("", "conn-1"), Subscription);
        assert_eq!(CostBasis::of_route(" ChatGPT-Codex ", ""), Subscription);
        assert_eq!(CostBasis::of_route("codex-subscription", ""), Subscription);
        assert_eq!(CostBasis::of_route("azure-openai", ""), Metered);
        assert_eq!(CostBasis::of_route("", "  "), Metered);
        // A synthetic completion reports no provider: the route decides.
        assert_eq!(Subscription.served_by(""), Subscription);
        assert_eq!(Metered.served_by(" "), Metered);
        // Otherwise the provider that actually served decides.
        assert_eq!(Metered.served_by("openai-codex-subscription"), Subscription);
        assert_eq!(Subscription.served_by("azure-openai"), Metered);
        assert_eq!(Metered.as_str(), "metered");
        assert_eq!(Subscription.as_str(), "subscription");
    }

    #[tokio::test]
    async fn subscription_calls_are_never_priced_from_list_prices() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        let price = |basis| cache.cost_usd_for(basis, "gpt-5.6-terra", 1_800, 120, 1_024, 256);
        // A disabled catalogue leaves a metered cost unavailable, while a
        // subscription call's cost is known either way.
        assert!(price(CostBasis::Metered).await.is_none());
        assert_eq!(price(CostBasis::Subscription).await, Some(0.0));

        cache.inner.write().await.rates = rates();
        let metered = price(CostBasis::Metered)
            .await
            .expect("priced against the default row");
        assert!(metered > 0.0, "a metered call is priced: {metered}");
        assert_eq!(
            price(CostBasis::Subscription).await,
            Some(0.0),
            "the default row's list price must never reach a subscription call"
        );
    }
}
