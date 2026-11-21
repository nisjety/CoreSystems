//! The price facts of the offline routing diff (EP-14 S10): the model keys cost-core prices, read from its public
//! `GET /api/v1/pricing` catalogue. inference-core keeps no price table of its own, and nothing is stored. The read
//! carries no credential, because cost-core serves that route outside its JWT wrapper, and each proposal reads it once.

use std::time::Duration;

use serde::Deserialize;
use tracing::warn;

/// The whole budget for one catalogue read. A proposal that waits this long for the price facts goes on without them,
/// and its diff reports every routed model as unpriced.
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);

/// Reads cost-core's price catalogue for the routing diff. Built from the same `COST_CORE_URL` the budget gate uses.
pub struct PriceSource {
    http: reqwest::Client,
    endpoint: String,
}

/// The part of cost-core's `GET /api/v1/pricing` body that the diff reads. Every other field is ignored.
#[derive(Deserialize)]
struct PricingBody {
    rates: Vec<Rate>,
}

#[derive(Deserialize)]
struct Rate {
    model: String,
}

impl PriceSource {
    /// A source that reads the catalogue at `cost_core_url`. `None` for an empty URL, which leaves the diff without
    /// price facts.
    #[must_use]
    pub fn new(cost_core_url: &str) -> Option<Self> {
        let base = cost_core_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return None;
        }
        let http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .ok()?;
        Some(Self {
            http,
            endpoint: format!("{base}/api/v1/pricing"),
        })
    }

    /// The model keys cost-core prices, trimmed and lowercased, or `None` when the catalogue cannot be read: a transport
    /// failure, a non-2xx status, or a body that is not the catalogue. It logs the status or a closed reason, never the
    /// body.
    pub async fn rate_keys(&self) -> Option<Vec<String>> {
        let response = match self.http.get(&self.endpoint).send().await {
            Ok(response) => response,
            Err(error) => {
                warn!(
                    timeout = error.is_timeout(),
                    connect = error.is_connect(),
                    "price catalogue unreachable; routing diff prices are unavailable"
                );
                return None;
            }
        };
        if !response.status().is_success() {
            warn!(
                status = %response.status(),
                "price catalogue non-2xx; routing diff prices are unavailable"
            );
            return None;
        }
        let Ok(body) = response.bytes().await else {
            warn!("price catalogue body unreadable; routing diff prices are unavailable");
            return None;
        };
        let keys = parse_rate_keys(&body);
        if keys.is_none() {
            warn!("price catalogue did not parse; routing diff prices are unavailable");
        }
        keys
    }
}

/// The model keys in a `GET /api/v1/pricing` body, trimmed and lowercased. `None` when the body is not that document.
#[must_use]
pub fn parse_rate_keys(body: &[u8]) -> Option<Vec<String>> {
    let document: PricingBody = serde_json::from_slice(body).ok()?;
    Some(
        document
            .rates
            .into_iter()
            .map(|rate| rate.model.trim().to_ascii_lowercase())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_keys_are_read_trimmed_and_lowercased() {
        let body = br#"{"rates":[{"model":" GPT-5-Nano ","input_per_million":0.05,"currency":"USD"},{"model":"default"}],"count":2}"#;
        assert_eq!(
            parse_rate_keys(body),
            Some(vec!["gpt-5-nano".to_owned(), "default".to_owned()])
        );
    }

    #[test]
    fn a_body_that_is_not_the_catalogue_is_no_catalogue() {
        assert_eq!(parse_rate_keys(br#"{"error":"internal"}"#), None);
        assert_eq!(
            parse_rate_keys(br#"{"rates":[{"input_per_million":0.1}]}"#),
            None
        );
        assert_eq!(parse_rate_keys(b"Internal Server Error"), None);
        assert_eq!(parse_rate_keys(b""), None);
    }

    #[test]
    fn an_empty_catalogue_is_a_catalogue_that_prices_nothing() {
        assert_eq!(
            parse_rate_keys(br#"{"rates":[],"count":0}"#),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_blank_url_has_no_source_and_a_trailing_slash_is_dropped() {
        assert!(PriceSource::new("   ").is_none());
        assert!(PriceSource::new("").is_none());
        let source = PriceSource::new(" http://cost-core:8089/ ").expect("a source");
        assert_eq!(source.endpoint, "http://cost-core:8089/api/v1/pricing");
    }

    #[tokio::test]
    async fn the_catalogue_is_read_without_a_credential() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/pricing"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "rates": [{ "model": "GPT-5-Nano" }], "count": 1 }),
            ))
            .mount(&server)
            .await;
        let source = PriceSource::new(&server.uri()).expect("a source");

        assert_eq!(
            source.rate_keys().await,
            Some(vec!["gpt-5-nano".to_owned()])
        );
        let requests = server.received_requests().await.expect("requests recorded");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].headers.get("authorization").is_none());
    }

    #[tokio::test]
    async fn a_server_error_is_no_catalogue_whatever_its_body() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v1/pricing"))
            .respond_with(
                wiremock::ResponseTemplate::new(500)
                    .set_body_string(r#"{"rates":[{"model":"gpt-5-nano"}]}"#),
            )
            .mount(&server)
            .await;
        let source = PriceSource::new(&server.uri()).expect("a source");

        assert_eq!(source.rate_keys().await, None);
    }
}
