//! The gateway's own `aud=cost-core` service credential for the Agent API key-turn budget check
//! (F-f10-23b, owner decisions D-f10-5 and DEC-M5).
//!
//! A key turn is checked against the org's spend ceilings before its model call. cost-core cannot
//! authenticate the key's inference bearer, so the gateway asks with ITS OWN credential and names
//! the key principal (`apikey:<key_id>`) in the body. This provider exchanges
//! `MODEL_GATEWAY_SERVICE_API_KEY` at Auth Core for a short-lived org-scoped `aud=cost-core` bearer
//! holding exactly one scope, `cost:budget:check` (granted by Control's principal registry,
//! F-f10-23a), and nothing else.
//!
//! Shaped as `session_terminal_auth.rs`: the token is cached per org until shortly before it
//! expires, concurrent mints for one org share one request, the client follows no redirect, and the
//! long-lived credential never appears in `Debug`. Only the bearer is cached, never a budget
//! verdict: every key turn asks cost-core again. Nothing here carries content.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::Mutex;

const COST_CORE_AUDIENCE: &str = "cost-core";
/// The one scope this provider ever asks for (cost-core's `ScopeBudgetCheck`).
pub(crate) const BUDGET_CHECK_SCOPE: &str = "cost:budget:check";
const MINT_REASON: &str = "model-gateway agent api key-turn budget check";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CostBudgetTokenError {
    #[error("cost-core budget-check service authentication is not configured: {0}")]
    Configuration(&'static str),
    #[error("cost-core budget-check credential requires an organization")]
    MissingOrganization,
    #[error("Auth Core budget-check token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the budget-check credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid cost-core budget-check token")]
    InvalidResponse,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

struct ProviderState {
    cache: Mutex<HashMap<String, CachedToken>>,
    in_flight: DashMap<String, Arc<Mutex<()>>>,
}

/// Mints the gateway's `aud=cost-core` bearer for one org, scope `cost:budget:check` only.
#[derive(Clone)]
pub(crate) struct CostBudgetTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    state: Arc<ProviderState>,
}

impl fmt::Debug for CostBudgetTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CostBudgetTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    #[serde(default)]
    audience: String,
}

/// The process-wide provider, built from the environment on first use. `None` while the service
/// credential is not configured: the key turn then has no service path (see
/// `budget::check_budget_for_key`), exactly as before this provider existed.
pub(crate) fn shared() -> Option<&'static CostBudgetTokenProvider> {
    static SHARED: OnceLock<Option<CostBudgetTokenProvider>> = OnceLock::new();
    SHARED
        .get_or_init(|| match CostBudgetTokenProvider::from_env() {
            Ok(provider) => Some(provider),
            Err(error) => {
                tracing::warn!(%error, "no cost-core budget-check service credential");
                None
            }
        })
        .as_ref()
}

impl CostBudgetTokenProvider {
    pub(crate) fn from_env() -> Result<Self, CostBudgetTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("MODEL_GATEWAY_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "model-gateway".to_owned());
        let credential = required_env("MODEL_GATEWAY_SERVICE_API_KEY")?;
        Self::from_service_credential(&auth_core_url, service_id, credential)
    }

    pub(crate) fn from_service_credential(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, CostBudgetTokenError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
            state: Arc::new(ProviderState {
                cache: Mutex::new(HashMap::new()),
                in_flight: DashMap::new(),
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(auth_core_url: String, service_id: &str, credential: &str) -> Self {
        Self::from_service_credential(&auth_core_url, service_id.to_owned(), credential.to_owned())
            .expect("test cost-core budget-check client")
    }

    /// The org's `aud=cost-core` `cost:budget:check` bearer, from the cache while it has more than
    /// [`REFRESH_SKEW`] left, otherwise freshly minted (one mint per org at a time).
    pub(crate) async fn budget_check_token(
        &self,
        org_id: &str,
    ) -> Result<String, CostBudgetTokenError> {
        let key = org_id.trim();
        if key.is_empty() {
            return Err(CostBudgetTokenError::MissingOrganization);
        }
        let key = key.to_owned();
        let now = Instant::now();
        {
            let mut cache = self.state.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.token.clone());
            }
        }

        let mint_lock = self
            .state
            .in_flight
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let mint_guard = mint_lock.lock().await;
        let now = Instant::now();
        {
            let cache = self.state.cache.lock().await;
            if let Some(cached) = cache.get(&key) {
                if cached.expires_at > now + REFRESH_SKEW {
                    let token = cached.token.clone();
                    drop(cache);
                    drop(mint_guard);
                    self.remove_unused_mint_lock(&key, &mint_lock);
                    return Ok(token);
                }
            }
        }

        let minted = match self.mint(&key).await {
            Ok(minted) => minted,
            Err(error) => {
                drop(mint_guard);
                self.remove_unused_mint_lock(&key, &mint_lock);
                return Err(error);
            }
        };
        let token = minted.token;
        let mut cache = self.state.cache.lock().await;
        let now = Instant::now();
        cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
        if !cache.contains_key(&key) && cache.len() >= MAX_CACHE_ENTRIES {
            if let Some(eviction_key) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.expires_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&eviction_key);
            }
        }
        cache.insert(
            key.clone(),
            CachedToken {
                token: token.clone(),
                expires_at: now + Duration::from_secs(minted.expires_in_seconds),
            },
        );
        drop(cache);
        drop(mint_guard);
        self.remove_unused_mint_lock(&key, &mint_lock);
        Ok(token)
    }

    async fn mint(&self, org_id: &str) -> Result<TokenResponse, CostBudgetTokenError> {
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| CostBudgetTokenError::Configuration("MODEL_GATEWAY_SERVICE_API_KEY"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{COST_CORE_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [BUDGET_CHECK_SCOPE],
                "reason": MINT_REASON,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(CostBudgetTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| CostBudgetTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || bundle.token.chars().any(char::is_whitespace)
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != COST_CORE_AUDIENCE
        {
            return Err(CostBudgetTokenError::InvalidResponse);
        }
        Ok(bundle)
    }

    fn remove_unused_mint_lock(&self, key: &str, expected: &Arc<Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.state
                .in_flight
                .remove_if(key, |_, current| Arc::ptr_eq(current, expected));
        }
    }
}

fn required_env(name: &'static str) -> Result<String, CostBudgetTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(CostBudgetTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::CostBudgetTokenProvider;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SERVICE_CREDENTIAL: &str = "fixture-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn token(audience: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "cost-budget-token",
            "expiresInSeconds": 300,
            "audience": audience
        }))
    }

    #[tokio::test]
    async fn mints_only_the_cost_core_budget_check_scope_once_and_reuses_it() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/cost-core/internal-token"))
            .and(header("x-service-id", "model-gateway"))
            .and(header("x-service-api-key", SERVICE_CREDENTIAL))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["cost:budget:check"],
                "reason": "model-gateway agent api key-turn budget check"
            })))
            .respond_with(token("cost-core"))
            .expect(1)
            .mount(&auth)
            .await;
        let provider =
            CostBudgetTokenProvider::new_for_test(auth.uri(), "model-gateway", SERVICE_CREDENTIAL);

        // Concurrent first asks share one mint; later asks hit the cache.
        let (first, second) = tokio::join!(
            provider.budget_check_token("org-a"),
            provider.budget_check_token("org-a")
        );
        assert_eq!(first.unwrap(), "cost-budget-token");
        assert_eq!(second.unwrap(), "cost-budget-token");
        assert_eq!(
            provider.budget_check_token(" org-a ").await.unwrap(),
            "cost-budget-token",
            "the bearer is minted once per org and reused"
        );
    }

    #[tokio::test]
    async fn refuses_a_token_for_another_audience_a_refusal_or_no_org() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/cost-core/internal-token"))
            .respond_with(token("session-core"))
            .mount(&auth)
            .await;
        let provider =
            CostBudgetTokenProvider::new_for_test(auth.uri(), "model-gateway", SERVICE_CREDENTIAL);
        assert!(provider.budget_check_token("org-a").await.is_err());
        assert!(provider.budget_check_token("  ").await.is_err());

        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .expect(2)
            .mount(&auth)
            .await;
        let provider =
            CostBudgetTokenProvider::new_for_test(auth.uri(), "model-gateway", SERVICE_CREDENTIAL);
        assert!(provider.budget_check_token("org-a").await.is_err());
        assert!(
            provider.budget_check_token("org-a").await.is_err(),
            "a refusal is not cached as a token"
        );
    }

    #[tokio::test]
    async fn does_not_follow_a_redirect_with_the_credential() {
        let auth = MockServer::start().await;
        let elsewhere = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/steal", elsewhere.uri())),
            )
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .respond_with(token("cost-core"))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let provider =
            CostBudgetTokenProvider::new_for_test(auth.uri(), "model-gateway", SERVICE_CREDENTIAL);
        assert!(provider.budget_check_token("org-a").await.is_err());
    }

    #[test]
    fn the_credential_never_appears_in_debug() {
        let provider = CostBudgetTokenProvider::new_for_test(
            "http://auth-core".to_owned(),
            "model-gateway",
            SERVICE_CREDENTIAL,
        );
        let rendered = format!("{provider:?}");
        assert!(!rendered.contains(SERVICE_CREDENTIAL), "{rendered}");
        assert!(rendered.contains("[REDACTED]"));
    }
}
