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
//! that is a prefix of the reported name, else the mandatory `default` row. The
//! default row is the ledger's fallback, not a price for the model: a call that
//! only the default row matches is reported as unpriced, so the stream states
//! no cost rather than a confident wrong one.
//!
//! Only metered calls are priced. A call served by the user's own `ChatGPT`
//! subscription ([`CostBasis::Subscription`]), or by the organisation's own
//! customer gateway ([`CostBasis::CustomerBilled`]), is never priced from API
//! list prices, whatever tokens it reports.

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

/// The served-by prefix of a customer-gateway call: Integration Core's `customer-gateway` registry id and a dot,
/// then the opaque connection id (`customer-gateway.<connection id>`).
const CUSTOMER_GATEWAY_PREFIX: &str = "customer-gateway.";

/// The billing label a customer-gateway call is recorded under on the usage envelope. Integration Core's
/// customer-gateway adapter logs the same label (`BILLING_LABEL`), and cost-core prices nothing under it.
const CUSTOMER_BILLED_LABEL: &str = "customer_connection";

/// The service tier a provider reports on `InferResponse.service_tier_used` when it served the call on its
/// priority tier.
pub const SERVICE_TIER_PRIORITY: &str = "priority";

/// The cost basis note a call carries when it was served on the priority tier (see [`Priced::note`]).
pub const PRIORITY_NOTE: &str = "priority";

/// The environment variable that sets the list-price multiplier of a call served on the priority tier.
pub const PRIORITY_TIER_COST_MULTIPLIER_ENV: &str = "PRIORITY_TIER_COST_MULTIPLIER";

/// The multiplier used when the variable is unset, empty or not a number.
const PRIORITY_TIER_COST_MULTIPLIER_DEFAULT: f64 = 2.0;

/// The bounds the multiplier is clamped to: a priority call is never priced below its list price and never
/// above five times it.
const PRIORITY_TIER_COST_MULTIPLIER_MIN: f64 = 1.0;
const PRIORITY_TIER_COST_MULTIPLIER_MAX: f64 = 5.0;

/// The multiplier a value of [`PRIORITY_TIER_COST_MULTIPLIER_ENV`] gives: the number, clamped to 1 through 5,
/// or 2.0 when the value is unset, empty or not a finite number.
#[must_use]
pub fn priority_tier_cost_multiplier_from(value: Option<&str>) -> f64 {
    value
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|parsed| parsed.is_finite())
        .unwrap_or(PRIORITY_TIER_COST_MULTIPLIER_DEFAULT)
        .clamp(
            PRIORITY_TIER_COST_MULTIPLIER_MIN,
            PRIORITY_TIER_COST_MULTIPLIER_MAX,
        )
}

/// The priority-tier multiplier of this process, read once from the environment.
#[must_use]
pub fn priority_tier_cost_multiplier() -> f64 {
    static MULTIPLIER: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *MULTIPLIER.get_or_init(|| {
        let raw = std::env::var(PRIORITY_TIER_COST_MULTIPLIER_ENV).ok();
        priority_tier_cost_multiplier_from(raw.as_deref())
    })
}

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
    /// Served by the organisation's own customer gateway (W4-BYO-5): an OpenAI-compatible endpoint it registered
    /// in Integration Core. The gateway bills the call, not the platform: the cost is 0.0 with no dollar figure,
    /// the token counts are usage and not platform spend, and API list prices never apply. A turn names a
    /// connection only while `MODEL_GATEWAY_BYO_GATEWAY` is on, so this basis is reached only then.
    CustomerBilled,
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

    /// The basis a call's request implies: customer-billed when the request names a customer-gateway connection
    /// (see [`Self::of_gateway`]), else the route's basis (see [`Self::of_route`]).
    #[must_use]
    pub fn of_request(
        provider_hint: &str,
        subscription_connection_id: &str,
        model_connection_id: &str,
    ) -> Self {
        if model_connection_id.trim().is_empty() {
            Self::of_route(provider_hint, subscription_connection_id)
        } else {
            Self::of_gateway(model_connection_id)
        }
    }

    /// The basis a call made through a customer-gateway connection implies: customer-billed when the connection
    /// id is not blank, else metered. The provider that actually serves the call still decides in
    /// [`Self::served_by`].
    #[must_use]
    pub fn of_gateway(model_connection_id: &str) -> Self {
        if model_connection_id.trim().is_empty() {
            Self::Metered
        } else {
            Self::CustomerBilled
        }
    }

    /// The basis once the serving provider is known. The provider that
    /// actually served decides; an empty provenance, from a completion that
    /// made no provider call of its own (a checked artifact's synthetic
    /// answer), keeps the route's basis. A customer-gateway route keeps its
    /// basis only when the customer gateway served the call: any other
    /// provider makes it metered, so a platform call is never labelled
    /// customer-billed.
    #[must_use]
    pub fn served_by(self, provider_used: &str) -> Self {
        if provider_used.trim().is_empty() {
            self
        } else if is_subscription_provider(provider_used) {
            Self::Subscription
        } else if self == Self::CustomerBilled && is_customer_gateway_provider(provider_used) {
            Self::CustomerBilled
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
            Self::CustomerBilled => CUSTOMER_BILLED_LABEL,
        }
    }
}

fn is_subscription_provider(provider: &str) -> bool {
    let provider = provider.trim();
    SUBSCRIPTION_PROVIDERS
        .iter()
        .any(|id| provider.eq_ignore_ascii_case(id))
}

/// Whether a served-by id names the customer gateway: the `customer-gateway.` prefix and a non-blank connection
/// id after it. The bare registry id names no connection, so it does not count.
fn is_customer_gateway_provider(provider: &str) -> bool {
    let provider = provider.trim();
    provider
        .get(..CUSTOMER_GATEWAY_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(CUSTOMER_GATEWAY_PREFIX))
        && !provider[CUSTOMER_GATEWAY_PREFIX.len()..].trim().is_empty()
}

/// The token counts of one call, as the provider reported them. Both cache
/// legs are already counted inside `input_tokens` (see
/// [`PricingCache::cost_usd`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenLegs {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
}

/// Where a [`Priced`] figure came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceSource {
    /// The catalogue lists the model under its own key.
    Exact,
    /// The longest catalogue key that prefixes the reported model name.
    Prefix,
    /// Only the catalogue's `default` row matched: the model's price is unknown.
    Default,
    /// The call was served by a subscription: no per-token price applies.
    Subscription,
    /// The call was served by the organisation's own customer gateway: the platform sets no per-token price.
    CustomerBilled,
    /// There is no catalogue to price against.
    Unavailable,
}

/// A call's cost split by what was billed. The four parts sum to the call's
/// total for any counts where the cache legs fit inside `input_tokens`, as a
/// provider reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CategoryCost {
    /// Uncached input tokens at the full input rate.
    pub input_usd: f64,
    pub cache_read_usd: f64,
    pub cache_write_usd: f64,
    pub output_usd: f64,
}

impl CategoryCost {
    #[must_use]
    pub fn total(&self) -> f64 {
        self.input_usd + self.cache_read_usd + self.cache_write_usd + self.output_usd
    }

    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            input_usd: self.input_usd + other.input_usd,
            cache_read_usd: self.cache_read_usd + other.cache_read_usd,
            cache_write_usd: self.cache_write_usd + other.cache_write_usd,
            output_usd: self.output_usd + other.output_usd,
        }
    }
}

/// The price of one call. `usd` is `None` when the model's price is unknown
/// (`source` says why), never a figure borrowed from the `default` row. A
/// subscription or customer-gateway call has `usd: Some(0.0)` and no
/// categories: nothing was billed per token, and a meter must not count the
/// zero as spend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Priced {
    pub usd: Option<f64>,
    pub by_category: Option<CategoryCost>,
    pub source: PriceSource,
    /// The cost basis note: [`PRIORITY_NOTE`] when the call was served on the priority tier and has a
    /// price (a metered price with the multiplier applied, or a subscription's known zero). `None` otherwise.
    pub note: Option<&'static str>,
}

impl Priced {
    const SUBSCRIPTION: Self = Self {
        usd: Some(0.0),
        by_category: None,
        source: PriceSource::Subscription,
        note: None,
    };

    const CUSTOMER_BILLED: Self = Self {
        usd: Some(0.0),
        by_category: None,
        source: PriceSource::CustomerBilled,
        note: None,
    };

    /// The known zero a call the platform does not bill per token carries: a subscription's or a customer
    /// gateway's. `None` for a metered call, which is priced from the catalogue.
    fn zero_for(basis: CostBasis) -> Option<Self> {
        match basis {
            CostBasis::Metered => None,
            CostBasis::Subscription => Some(Self::SUBSCRIPTION),
            CostBasis::CustomerBilled => Some(Self::CUSTOMER_BILLED),
        }
    }

    const fn unpriced(source: PriceSource) -> Self {
        Self {
            usd: None,
            by_category: None,
            source,
            note: None,
        }
    }
}

/// What the catalogue lists a model at, per one million tokens, before any call
/// is priced. Only a row that names the model has one (see
/// [`RateSnapshot::list_price`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ListPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub currency: String,
    /// [`PriceSource::Exact`] or [`PriceSource::Prefix`].
    pub source: PriceSource,
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
    #[serde(default)]
    currency: String,
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

    /// A cache that already holds one rate row, so a test can price a call without cost-core.
    #[cfg(test)]
    pub(crate) fn with_rate_for_test(model: &str, input_per_million: f64, output_per_million: f64) -> Self {
        let cache = Self::new(None, reqwest::Client::new());
        let mut inner = cache
            .inner
            .try_write()
            .expect("a fresh cache is not locked");
        inner.rates = vec![Rate {
            model: model.trim().to_lowercase(),
            input_per_million,
            output_per_million,
            currency: "USD".to_owned(),
        }];
        inner.fetched_at = Some(Instant::now());
        drop(inner);
        cache
    }

    /// Price one call. A subscription or customer-gateway call is never priced
    /// from API list prices. A metered call is priced against the catalogue row that matches
    /// its model; when only the `default` row matches, or there is no catalogue
    /// (pricing disabled, or cost-core unreachable with a cold cache), `usd` is
    /// `None` so the caller emits a null cost instead of a fabricated one.
    ///
    /// The cache legs in `legs` are the same ones already folded into
    /// `input_tokens` (see `inference-core`'s `total_input_tokens` and
    /// `openai_cache_tokens`): pass 0 for a request that reported no cache
    /// usage, which reproduces the pre-cache-pricing result exactly. Each leg
    /// is re-priced at the model's [`cache_rates`]: a read below the input
    /// rate, and a write at the input rate unless the model bills cache writes
    /// (GPT-5.6 and later).
    pub async fn priced(&self, basis: CostBasis, model: &str, legs: TokenLegs) -> Priced {
        self.priced_at_tier(basis, model, legs, false).await
    }

    /// [`Self::priced`] for a call that may have been served on the priority tier (`priority`, from its
    /// `service_tier_used`). A metered call on that tier is priced at its list price times
    /// [`priority_tier_cost_multiplier`] and carries [`PRIORITY_NOTE`]. A subscription call has no list price
    /// to multiply, so it keeps its zero and carries the note.
    pub async fn priced_at_tier(
        &self,
        basis: CostBasis,
        model: &str,
        legs: TokenLegs,
        priority: bool,
    ) -> Priced {
        if let Some(zero) = Priced::zero_for(basis) {
            return Priced {
                note: priority.then_some(PRIORITY_NOTE),
                ..zero
            };
        }
        self.ensure_fresh().await;
        let inner = self.inner.read().await;
        price(&inner.rates, model, legs, priority)
    }

    /// The catalogue as of now, for a caller that prices from a synchronous
    /// context. One refresh here, then [`RateSnapshot::priced`] never waits.
    pub async fn snapshot(&self) -> RateSnapshot {
        self.ensure_fresh().await;
        let inner = self.inner.read().await;
        RateSnapshot {
            rates: Arc::from(inner.rates.clone()),
        }
    }

    /// The USD cost of a metered call: [`Self::priced`]'s `usd`.
    pub async fn cost_usd(
        &self,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
    ) -> Option<f64> {
        self.cost_usd_for(
            CostBasis::Metered,
            model,
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        )
        .await
    }

    /// [`Self::priced`]'s `usd` for a call made on `basis`: a subscription
    /// call's cost is a known `Some(0.0)`, not the unavailable `None`,
    /// whatever tokens it reports.
    pub async fn cost_usd_for(
        &self,
        basis: CostBasis,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_input_tokens: i64,
        cache_creation_input_tokens: i64,
    ) -> Option<f64> {
        self.priced(
            basis,
            model,
            TokenLegs {
                input_tokens,
                output_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
            },
        )
        .await
        .usd
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

/// A frozen copy of the catalogue that prices without waiting. A relay that
/// maps events in a synchronous closure takes one per run and prices every
/// round against it.
#[derive(Clone)]
pub struct RateSnapshot {
    rates: Arc<[Rate]>,
}

impl RateSnapshot {
    /// [`PricingCache::priced`] against this snapshot.
    #[must_use]
    pub fn priced(&self, basis: CostBasis, model: &str, legs: TokenLegs) -> Priced {
        self.priced_at_tier(basis, model, legs, false)
    }

    /// [`Self::priced_at_tier`] against this snapshot.
    #[must_use]
    pub fn priced_at_tier(
        &self,
        basis: CostBasis,
        model: &str,
        legs: TokenLegs,
        priority: bool,
    ) -> Priced {
        if let Some(zero) = Priced::zero_for(basis) {
            return Priced {
                note: priority.then_some(PRIORITY_NOTE),
                ..zero
            };
        }
        price(&self.rates, model, legs, priority)
    }

    /// The list price of a model the catalogue names: under its own key, or under a
    /// key that prefixes it, by the same resolver as [`Self::priced`]. A subscription
    /// or customer-gateway call has no list price, and neither does a model only the
    /// `default` row matches.
    #[must_use]
    pub fn list_price(&self, basis: CostBasis, model: &str) -> Option<ListPrice> {
        if basis != CostBasis::Metered {
            return None;
        }
        let (rate, source) = match lookup(&self.rates, model)? {
            Lookup::Exact(rate) => (rate, PriceSource::Exact),
            Lookup::Prefix(rate) => (rate, PriceSource::Prefix),
            Lookup::Default => return None,
        };
        Some(ListPrice {
            input_per_million: rate.input_per_million,
            output_per_million: rate.output_per_million,
            currency: rate.currency.clone(),
            source,
        })
    }
}

/// Price a metered call against the catalogue. The `default` row prices
/// nothing: a model only it matches has an unknown price.
fn price(rates: &[Rate], model: &str, legs: TokenLegs, priority: bool) -> Priced {
    let (rate, source) = match lookup(rates, model) {
        None => return Priced::unpriced(PriceSource::Unavailable),
        Some(Lookup::Default) => return Priced::unpriced(PriceSource::Default),
        Some(Lookup::Exact(rate)) => (rate, PriceSource::Exact),
        Some(Lookup::Prefix(rate)) => (rate, PriceSource::Prefix),
    };
    // A call served on the priority tier is priced at the list price times the multiplier. Every cache leg
    // is priced off the same input rate, so the multiplier reaches each part of the cost.
    let multiplied = priority.then(|| at_multiplier(rate, priority_tier_cost_multiplier()));
    let rate = multiplied.as_ref().unwrap_or(rate);
    let cache = cache_rates(model);
    Priced {
        usd: Some(cost_from_rate(
            rate,
            cache,
            legs.input_tokens,
            legs.output_tokens,
            legs.cache_read_input_tokens,
            legs.cache_creation_input_tokens,
        )),
        by_category: Some(cost_categories(rate, cache, legs)),
        source,
        note: priority.then_some(PRIORITY_NOTE),
    }
}

/// `rate` with both list prices multiplied by `multiplier`. The model key and currency are kept.
fn at_multiplier(rate: &Rate, multiplier: f64) -> Rate {
    Rate {
        model: rate.model.clone(),
        input_per_million: rate.input_per_million * multiplier,
        output_per_million: rate.output_per_million * multiplier,
        currency: rate.currency.clone(),
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
    let input = tokens_f64(input_tokens);
    let output = tokens_f64(output_tokens);
    let cache_read = tokens_f64(cache_read_input_tokens);
    let cache_write = tokens_f64(cache_creation_input_tokens);
    input / 1_000_000.0 * rate.input_per_million + output / 1_000_000.0 * rate.output_per_million
        - cache_read / 1_000_000.0 * rate.input_per_million * (1.0 - cache.read)
        + cache_write / 1_000_000.0 * rate.input_per_million * (cache.write - 1.0)
}

/// A token count as f64. Counts comfortably fit in u32; the u32→f64
/// conversion is lossless (avoids clippy::cast_precision_loss on a raw i64
/// cast).
fn tokens_f64(tokens: i64) -> f64 {
    f64::from(u32::try_from(tokens.max(0)).unwrap_or(u32::MAX))
}

/// The same price as [`cost_from_rate`], split by what was billed: uncached
/// input at the full rate, each cache leg at its own rate, and output. The
/// uncached input is `input - read - write`, saturating at zero.
fn cost_categories(rate: &Rate, cache: CacheRates, legs: TokenLegs) -> CategoryCost {
    let uncached = legs
        .input_tokens
        .max(0)
        .saturating_sub(legs.cache_read_input_tokens.max(0))
        .saturating_sub(legs.cache_creation_input_tokens.max(0))
        .max(0);
    let input_rate = rate.input_per_million / 1_000_000.0;
    CategoryCost {
        input_usd: tokens_f64(uncached) * input_rate,
        cache_read_usd: tokens_f64(legs.cache_read_input_tokens) * input_rate * cache.read,
        cache_write_usd: tokens_f64(legs.cache_creation_input_tokens) * input_rate * cache.write,
        output_usd: tokens_f64(legs.output_tokens) * rate.output_per_million / 1_000_000.0,
    }
}

/// How a model name matched the catalogue.
enum Lookup<'a> {
    Exact(&'a Rate),
    Prefix(&'a Rate),
    /// Only the mandatory `default` row: the ledger's fallback, not the
    /// model's price.
    Default,
}

/// Resolve a model name to a rate: exact key, else the longest prefix key, else
/// the `default` row.
fn lookup<'a>(rates: &'a [Rate], model: &str) -> Option<Lookup<'a>> {
    let key = model.trim().to_lowercase();
    if !key.is_empty() {
        if let Some(rate) = rates.iter().find(|r| r.model == key) {
            return Some(if key == DEFAULT_MODEL_KEY {
                Lookup::Default
            } else {
                Lookup::Exact(rate)
            });
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
        if let Some(rate) = best {
            return Some(Lookup::Prefix(rate));
        }
    }
    rates
        .iter()
        .find(|r| r.model == DEFAULT_MODEL_KEY)
        .map(|_| Lookup::Default)
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
                currency: "USD".into(),
            },
            Rate {
                model: "gpt-4o-mini".into(),
                input_per_million: 0.15,
                output_per_million: 0.60,
                currency: "USD".into(),
            },
            Rate {
                model: "claude-sonnet".into(),
                input_per_million: 3.0,
                output_per_million: 15.0,
                currency: "USD".into(),
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
            currency: "USD".into(),
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
            currency: "USD".into(),
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
            currency: "USD".into(),
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
            currency: "USD".into(),
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
            currency: "USD".into(),
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
        assert!(
            matches!(lookup(&r, "gpt-4o-mini"), Some(Lookup::Exact(rate)) if rate.model == "gpt-4o-mini")
        );
        assert!(
            matches!(lookup(&r, "claude-sonnet-4-6"), Some(Lookup::Prefix(rate)) if rate.model == "claude-sonnet")
        );
        assert!(matches!(
            lookup(&r, "totally-unknown"),
            Some(Lookup::Default)
        ));
        // The default row's own key is not a model: it resolves as the fallback.
        assert!(matches!(lookup(&r, "default"), Some(Lookup::Default)));
        assert!(matches!(lookup(&r, "  "), Some(Lookup::Default)));
        assert!(lookup(&r[1..], "totally-unknown").is_none());
    }

    #[tokio::test]
    async fn unknown_model_is_unpriced_not_default_priced() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        cache.inner.write().await.rates = rates();
        let legs = TokenLegs {
            input_tokens: 1_000,
            output_tokens: 500,
            ..TokenLegs::default()
        };

        let unknown = cache
            .priced(
                CostBasis::Metered,
                "a-model-the-catalogue-does-not-list",
                legs,
            )
            .await;
        assert_eq!(unknown, Priced::unpriced(PriceSource::Default));
        for model in ["", "  ", "default"] {
            let got = cache.priced(CostBasis::Metered, model, legs).await;
            assert_eq!(got.usd, None, "{model:?}");
            assert_eq!(got.source, PriceSource::Default, "{model:?}");
        }
        assert_eq!(
            cache
                .cost_usd_for(CostBasis::Metered, "gpt-5.6-terra", 1_000, 500, 0, 0)
                .await,
            None,
            "the thin wrapper agrees: no default-row cost"
        );

        let want = 1_000.0 / 1_000_000.0 * 0.15 + 500.0 / 1_000_000.0 * 0.60;
        let exact = cache.priced(CostBasis::Metered, "gpt-4o-mini", legs).await;
        assert_eq!(exact.source, PriceSource::Exact);
        assert!((exact.usd.unwrap() - want).abs() < 1e-12);
        let prefix = cache
            .priced(CostBasis::Metered, "claude-sonnet-4-6", legs)
            .await;
        assert_eq!(prefix.source, PriceSource::Prefix);
        assert!(prefix.usd.is_some() && prefix.by_category.is_some());
    }

    #[tokio::test]
    async fn list_price_names_only_the_models_the_catalogue_lists() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        cache.inner.write().await.rates = rates();
        let snapshot = cache.snapshot().await;

        let exact = snapshot
            .list_price(CostBasis::Metered, "gpt-4o-mini")
            .expect("an exact key is listed");
        assert_eq!(exact.source, PriceSource::Exact);
        assert_eq!(
            (exact.input_per_million, exact.output_per_million),
            (0.15, 0.60)
        );
        assert_eq!(exact.currency, "USD");
        let prefix = snapshot
            .list_price(CostBasis::Metered, "claude-sonnet-5-5")
            .expect("a prefix key is listed");
        assert_eq!(prefix.source, PriceSource::Prefix);
        assert_eq!(
            (prefix.input_per_million, prefix.output_per_million),
            (3.0, 15.0)
        );
        // Only the default row matches these, so the catalogue names no price for them.
        for model in ["a-model-the-catalogue-does-not-list", "default", "", "  "] {
            assert_eq!(
                snapshot.list_price(CostBasis::Metered, model),
                None,
                "{model:?}"
            );
        }
        // A subscription call has no list price, even for a model the catalogue lists.
        assert_eq!(
            snapshot.list_price(CostBasis::Subscription, "gpt-4o-mini"),
            None
        );
        let empty = PricingCache::new(None, reqwest::Client::new())
            .snapshot()
            .await;
        assert_eq!(empty.list_price(CostBasis::Metered, "gpt-4o-mini"), None);
    }

    #[tokio::test]
    async fn a_cold_catalogue_is_unavailable_and_a_subscription_is_a_known_zero() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        let legs = TokenLegs {
            input_tokens: 10,
            ..TokenLegs::default()
        };
        assert_eq!(
            cache.priced(CostBasis::Metered, "gpt-4o-mini", legs).await,
            Priced::unpriced(PriceSource::Unavailable)
        );
        let subscription = cache
            .priced(CostBasis::Subscription, "gpt-5.6-terra", legs)
            .await;
        assert_eq!(subscription.usd, Some(0.0));
        assert_eq!(subscription.by_category, None);
        assert_eq!(subscription.source, PriceSource::Subscription);
    }

    #[test]
    fn category_costs_sum_to_total() {
        let rate = Rate {
            model: "claude-sonnet".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
            currency: "USD".into(),
        };
        // (input, output, cache read, cache write): the legs always fit inside
        // the input, as a provider reports them.
        let counts = [
            (0, 0, 0, 0),
            (1_000, 500, 0, 0),
            (8_520, 42, 8_000, 0),
            (2_000, 50, 1_024, 512),
            (2_000, 50, 0, 512),
            (2_000, 50, 2_000, 0),
            (1_566, 2, 1_408, 0),
            (4_000_000, 90_000, 1_000_000, 2_500_000),
        ];
        for model in [
            "claude-sonnet-4-6",
            "gpt-4o-mini",
            "gpt-5.2",
            "gpt-5.6",
            "gpt-5.6-terra",
        ] {
            for (input, output, read, write) in counts {
                let legs = TokenLegs {
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_input_tokens: read,
                    cache_creation_input_tokens: write,
                };
                let cache = cache_rates(model);
                let total = cost_from_rate(&rate, cache, input, output, read, write);
                let parts = cost_categories(&rate, cache, legs);
                assert!(
                    (parts.total() - total).abs() < 1e-9,
                    "{model} {legs:?}: parts {parts:?} sum to {}, total {total}",
                    parts.total()
                );
            }
        }
    }

    #[test]
    fn category_costs_split_by_what_was_billed() {
        let rate = Rate {
            model: "default".into(),
            input_per_million: 3.0,
            output_per_million: 15.0,
            currency: "USD".into(),
        };
        let parts = cost_categories(
            &rate,
            cache_rates("gpt-5.6"),
            TokenLegs {
                input_tokens: 2_000,
                output_tokens: 50,
                cache_read_input_tokens: 1_024,
                cache_creation_input_tokens: 512,
            },
        );
        let want = CategoryCost {
            input_usd: 464.0 / 1_000_000.0 * 3.0,
            cache_read_usd: 1_024.0 / 1_000_000.0 * 3.0 * 0.1,
            cache_write_usd: 512.0 / 1_000_000.0 * 3.0 * 1.25,
            output_usd: 50.0 / 1_000_000.0 * 15.0,
        };
        for (got, want) in [
            (parts.input_usd, want.input_usd),
            (parts.cache_read_usd, want.cache_read_usd),
            (parts.cache_write_usd, want.cache_write_usd),
            (parts.output_usd, want.output_usd),
        ] {
            assert!((got - want).abs() < 1e-12, "got {parts:?}, want {want:?}");
        }
        // Legs larger than the input saturate the uncached part at zero
        // instead of going negative.
        let over = cost_categories(
            &rate,
            cache_rates("claude-sonnet-4-6"),
            TokenLegs {
                input_tokens: 100,
                cache_read_input_tokens: 80,
                cache_creation_input_tokens: 80,
                ..TokenLegs::default()
            },
        );
        assert_eq!(over.input_usd, 0.0);
    }

    #[tokio::test]
    async fn a_snapshot_prices_like_the_cache() {
        let cache = PricingCache::new(None, reqwest::Client::new());
        cache.inner.write().await.rates = rates();
        let snapshot = cache.snapshot().await;
        let legs = TokenLegs {
            input_tokens: 8_520,
            output_tokens: 42,
            cache_read_input_tokens: 8_000,
            cache_creation_input_tokens: 0,
        };
        for (basis, model) in [
            (CostBasis::Metered, "claude-sonnet-4-6"),
            (CostBasis::Metered, "gpt-4o-mini"),
            (CostBasis::Metered, "gpt-5.6-terra"),
            (CostBasis::Subscription, "gpt-5.6-terra"),
        ] {
            assert_eq!(
                snapshot.priced(basis, model, legs),
                cache.priced(basis, model, legs).await,
                "{basis:?} {model}"
            );
        }
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
        assert!(
            price(CostBasis::Metered).await.is_none(),
            "only the default row matches this model: it has no price"
        );
        assert_eq!(
            price(CostBasis::Subscription).await,
            Some(0.0),
            "the default row's list price must never reach a subscription call"
        );
        let metered = cache
            .cost_usd_for(
                CostBasis::Metered,
                "claude-sonnet-4-6",
                1_800,
                120,
                1_024,
                256,
            )
            .await
            .expect("a listed model is priced");
        assert!(metered > 0.0, "a metered call is priced: {metered}");
    }
}
