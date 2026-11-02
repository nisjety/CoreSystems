//! The list price a `/v1/models` chat entry carries (EP-13-S1).
//!
//! The price is read from the catalogue the gateway already caches for cost
//! ([`crate::pricing`]), by model id at read time. It is never copied into
//! inference-core's model record, and cost-core stays the only owner of the
//! rates. The switch `MODEL_GATEWAY_CATALOGUE_PRICES` is off by default: with it
//! off no catalogue is fetched for a listing and the JSON is what it was before.

use serde::Serialize;

use crate::pricing::{CostBasis, PriceSource, PricingCache, RateSnapshot};

/// The switch. Only the literal `true` turns the list prices on.
pub const CATALOGUE_PRICES_ENV: &str = "MODEL_GATEWAY_CATALOGUE_PRICES";

/// Whether listings carry list prices. Read on every listing, so a flipped
/// switch takes effect on the next request.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(CATALOGUE_PRICES_ENV).ok().as_deref())
}

fn enabled_from(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// The catalogue one listing is priced against. It is taken once per request,
/// because a per-model refresh would fetch again for every model while the
/// catalogue is empty. `None` while the switch is off: nothing is priced and
/// nothing is fetched.
pub async fn snapshot_for_listing(pricing: &PricingCache) -> Option<RateSnapshot> {
    if enabled() {
        Some(pricing.snapshot().await)
    } else {
        None
    }
}

/// A chat model's list price on `/v1/models`, per one million tokens. `source`
/// says whether the catalogue lists the model under its own key (`exact`) or
/// under a shorter key that prefixes it (`prefix`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub currency: String,
    pub source: &'static str,
}

/// The list price of one listed model, or none. Only chat models are priced,
/// because the picker shows this figure for chat turns. A subscription model
/// has no per-token list price, and a model only the `default` row matches has
/// an unknown one.
#[must_use]
pub fn model_price(
    rates: &RateSnapshot,
    id: &str,
    provider: &str,
    modality: &str,
) -> Option<ModelPrice> {
    if modality != "chat" {
        return None;
    }
    let price = rates.list_price(CostBasis::of_route(provider, ""), id)?;
    let source = match price.source {
        PriceSource::Exact => "exact",
        PriceSource::Prefix => "prefix",
        PriceSource::Default | PriceSource::Subscription | PriceSource::Unavailable => return None,
    };
    // A rate with no currency would be a bare number the picker has to guess at.
    if price.currency.trim().is_empty() {
        return None;
    }
    Some(ModelPrice {
        input_per_million: price.input_per_million,
        output_per_million: price.output_per_million,
        currency: price.currency,
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::{routing::get, Json, Router};
    use serial_test::serial;

    use super::*;

    #[test]
    fn the_switch_is_on_only_for_the_literal_true() {
        assert!(!enabled_from(None));
        for raw in ["", "1", "yes", "True", "TRUE", "false", "on"] {
            assert!(!enabled_from(Some(raw)), "{raw:?}");
        }
        assert!(enabled_from(Some("true")));
        assert!(enabled_from(Some(" true ")));
    }

    /// A cost-core stand-in that serves `body` on `/api/v1/pricing` and counts the fetches.
    async fn cost_core(body: serde_json::Value) -> (PricingCache, Arc<AtomicUsize>) {
        let fetches = Arc::new(AtomicUsize::new(0));
        let counter = fetches.clone();
        let app = Router::new().route(
            "/api/v1/pricing",
            get(move || {
                let counter = counter.clone();
                let body = body.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Json(body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let addr = listener.local_addr().expect("the bound address");
        tokio::spawn(async move { axum::serve(listener, app).await });
        let cache = PricingCache::new(Some(format!("http://{addr}")), reqwest::Client::new());
        (cache, fetches)
    }

    fn catalogue() -> serde_json::Value {
        serde_json::json!({ "rates": [
            { "model": "default", "input_per_million": 3.0, "output_per_million": 15.0, "currency": "USD" },
            { "model": "claude-sonnet", "input_per_million": 3.0, "output_per_million": 15.0, "currency": "USD" },
            { "model": "gpt-4o-mini", "input_per_million": 0.15, "output_per_million": 0.6, "currency": "USD" },
        ]})
    }

    #[tokio::test]
    #[serial]
    async fn list_models_takes_no_catalogue_while_the_switch_is_off() {
        std::env::remove_var(CATALOGUE_PRICES_ENV);
        let (cache, fetches) = cost_core(catalogue()).await;
        assert!(snapshot_for_listing(&cache).await.is_none());
        assert_eq!(fetches.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[serial]
    async fn list_models_takes_one_catalogue_per_listing() {
        std::env::set_var(CATALOGUE_PRICES_ENV, "true");
        // An empty catalogue is fetched again by every pricing call, so pricing
        // model by model would show up as extra fetches here.
        let (cache, fetches) = cost_core(serde_json::json!({ "rates": [] })).await;
        let rates = snapshot_for_listing(&cache)
            .await
            .expect("the switch is on");
        for id in ["claude-sonnet-5-5", "gpt-4o-mini", "gpt-5.6-terra"] {
            assert_eq!(model_price(&rates, id, "anthropic", "chat"), None);
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        std::env::remove_var(CATALOGUE_PRICES_ENV);
    }

    #[tokio::test]
    async fn list_models_prices_exact_and_prefix_matches_in_the_catalogue_currency() {
        let (cache, _) = cost_core(catalogue()).await;
        let rates = cache.snapshot().await;
        let exact = model_price(&rates, "gpt-4o-mini", "openai", "chat").expect("an exact key");
        assert_eq!(
            exact,
            ModelPrice {
                input_per_million: 0.15,
                output_per_million: 0.6,
                currency: "USD".to_owned(),
                source: "exact",
            }
        );
        let prefix =
            model_price(&rates, "claude-sonnet-5-5", "anthropic", "chat").expect("a prefix key");
        assert_eq!(prefix.source, "prefix");
        assert_eq!(
            (prefix.input_per_million, prefix.output_per_million),
            (3.0, 15.0)
        );
        // A dated snapshot of a listed model is priced by its prefix.
        assert_eq!(
            model_price(&rates, "gpt-4o-mini-2024-07-18", "openai", "chat").map(|p| p.source),
            Some("prefix")
        );
        // Not a chat model, or served by a subscription: no list price.
        assert_eq!(model_price(&rates, "gpt-4o-mini", "openai", "image"), None);
        assert_eq!(
            model_price(&rates, "gpt-4o-mini", "openai-codex-subscription", "chat"),
            None
        );
    }

    #[tokio::test]
    async fn list_models_never_prices_from_the_default_row() {
        let (cache, _) = cost_core(catalogue()).await;
        let rates = cache.snapshot().await;
        for id in ["a-model-the-catalogue-does-not-list", "default"] {
            assert_eq!(model_price(&rates, id, "azure", "chat"), None, "{id}");
        }
    }
}
