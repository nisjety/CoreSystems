use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::Mutex;

const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum TokenError {
    #[error("Quarry service-principal authentication is not configured: {0}")]
    Configuration(&'static str),
    #[error("Quarry organization is required")]
    MissingOrganization,
    #[error("Auth Core token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the Quarry credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid Quarry token response")]
    InvalidResponse,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CacheKey {
    auth_core_url: String,
    service_id: String,
    org_id: String,
    scopes: Vec<String>,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

struct ProviderState {
    cache: Mutex<HashMap<CacheKey, CachedToken>>,
    in_flight: DashMap<CacheKey, Arc<Mutex<()>>>,
    max_cache_entries: usize,
}

type SharedState = Arc<ProviderState>;
static PROCESS_STATE: OnceLock<SharedState> = OnceLock::new();

/// Process-shared service-principal token provider. `execution-core` creates
/// web-tool clients per dispatch, so the shared cache prevents a token mint on
/// every tool call while still partitioning tokens by org and scope set.
#[derive(Clone)]
pub(crate) struct TokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    state: SharedState,
}

impl fmt::Debug for TokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenProvider")
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

impl TokenProvider {
    pub(crate) fn from_env() -> Result<Self, TokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("EXECUTION_CORE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "execution-core".to_owned());
        let credential = required_env("EXECUTION_CORE_SERVICE_API_KEY")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
            state: PROCESS_STATE
                .get_or_init(|| {
                    Arc::new(ProviderState {
                        cache: Mutex::new(HashMap::new()),
                        in_flight: DashMap::new(),
                        max_cache_entries: MAX_CACHE_ENTRIES,
                    })
                })
                .clone(),
        })
    }

    #[cfg(test)]
    fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self::new_for_test_with_cache_limit(
            auth_core_url,
            service_id,
            credential,
            MAX_CACHE_ENTRIES,
        )
    }

    #[cfg(test)]
    fn new_for_test_with_cache_limit(
        auth_core_url: &str,
        service_id: &str,
        credential: &str,
        max_cache_entries: usize,
    ) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("test token client"),
            state: Arc::new(ProviderState {
                cache: Mutex::new(HashMap::new()),
                in_flight: DashMap::new(),
                max_cache_entries: max_cache_entries.max(1),
            }),
        }
    }

    pub(crate) async fn token(&self, org_id: &str, scopes: &[&str]) -> Result<String, TokenError> {
        let key = self.cache_key(org_id, scopes)?;
        let now = Instant::now();
        {
            let mut cache = self.state.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
            if let Some(cached) = cache.get(&key) {
                if cached.expires_at > now + REFRESH_SKEW {
                    return Ok(cached.token.clone());
                }
            }
            cache.remove(&key);
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
        if !cache.contains_key(&key) && cache.len() >= self.state.max_cache_entries {
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

    pub(crate) async fn invalidate_if_matches(
        &self,
        org_id: &str,
        scopes: &[&str],
        rejected_token: &str,
    ) -> Result<(), TokenError> {
        let key = self.cache_key(org_id, scopes)?;
        let mut cache = self.state.cache.lock().await;
        let matches_rejected = cache
            .get(&key)
            .is_some_and(|cached| cached.token == rejected_token);
        if matches_rejected {
            cache.remove(&key);
        }
        Ok(())
    }

    async fn mint(&self, key: &CacheKey) -> Result<TokenResponse, TokenError> {
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| TokenError::Configuration("EXECUTION_CORE_SERVICE_API_KEY"))?;
        credential.set_sensitive(true);

        let response = self
            .http
            .post(format!("{}/api/quarry/internal-token", self.auth_core_url))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": &key.org_id,
                "scopes": &key.scopes,
                "reason": "execution-core Quarry access",
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(TokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| TokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != "quarry"
        {
            return Err(TokenError::InvalidResponse);
        }
        Ok(bundle)
    }

    fn cache_key(&self, org_id: &str, scopes: &[&str]) -> Result<CacheKey, TokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(TokenError::MissingOrganization);
        }
        let mut scopes = scopes
            .iter()
            .map(|scope| scope.trim().to_owned())
            .filter(|scope| !scope.is_empty())
            .collect::<Vec<_>>();
        scopes.sort_unstable();
        scopes.dedup();
        if scopes.is_empty() {
            return Err(TokenError::Configuration("Quarry token scopes"));
        }
        Ok(CacheKey {
            auth_core_url: self.auth_core_url.clone(),
            service_id: self.service_id.clone(),
            org_id: org_id.to_owned(),
            scopes,
        })
    }

    fn remove_unused_mint_lock(&self, key: &CacheKey, expected: &Arc<Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.state
                .in_flight
                .remove_if(key, |_, current| Arc::ptr_eq(current, expected));
        }
    }

    #[cfg(test)]
    async fn cached_entry_count(&self) -> usize {
        self.state.cache.lock().await.len()
    }
}

fn required_env(name: &'static str) -> Result<String, TokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(TokenError::Configuration(name))
}

#[derive(Clone)]
pub(crate) enum TokenSource {
    Static(String),
    ServicePrincipal(TokenProvider),
}

impl fmt::Debug for TokenSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(_) => formatter.write_str("Static([REDACTED])"),
            Self::ServicePrincipal(provider) => formatter
                .debug_tuple("ServicePrincipal")
                .field(provider)
                .finish(),
        }
    }
}

impl TokenSource {
    pub(crate) fn from_env() -> Result<Self, TokenError> {
        TokenProvider::from_env().map(Self::ServicePrincipal)
    }

    pub(crate) async fn token(&self, org_id: &str, scopes: &[&str]) -> Result<String, TokenError> {
        match self {
            Self::Static(token) => Ok(token.clone()),
            Self::ServicePrincipal(provider) => provider.token(org_id, scopes).await,
        }
    }

    pub(crate) async fn invalidate_if_matches(
        &self,
        org_id: &str,
        scopes: &[&str],
        rejected_token: &str,
    ) -> Result<(), TokenError> {
        match self {
            Self::Static(_) => Ok(()),
            Self::ServicePrincipal(provider) => {
                provider
                    .invalidate_if_matches(org_id, scopes, rejected_token)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_execution_identity_token_and_caches_by_org_and_scope() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", "execution-core-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-b",
                "scopes": ["browser:execute"],
                "reason": "execution-core Quarry access"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "org-b-token",
                "expiresInSeconds": 300,
                "audience": "quarry"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider =
            TokenProvider::new_for_test(&auth.uri(), "execution-core", "execution-core-secret");
        assert_eq!(
            provider.token("org-b", &["browser:execute"]).await.unwrap(),
            "org-b-token"
        );
        assert_eq!(
            provider.token("org-b", &["browser:execute"]).await.unwrap(),
            "org-b-token"
        );
    }

    #[tokio::test]
    async fn rejects_empty_org_before_contacting_auth_core() {
        let auth = MockServer::start().await;
        let provider = TokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");
        let error = provider
            .token("  ", &["search:read"])
            .await
            .expect_err("empty org must fail closed");
        assert!(error.to_string().contains("organization is required"));
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn token_client_refuses_cross_host_redirects() {
        let sink = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&sink)
            .await;
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/sink", sink.uri())),
            )
            .mount(&auth)
            .await;
        let provider = TokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");

        let error = provider
            .token("org-a", &["browser:execute"])
            .await
            .expect_err("redirect must not be followed");
        assert!(matches!(error, TokenError::Refused(status) if status.as_u16() == 307));
    }

    #[tokio::test]
    async fn concurrent_cache_misses_share_one_token_mint() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(50))
                    .set_body_json(serde_json::json!({
                        "token": "shared-token",
                        "expiresInSeconds": 300,
                        "audience": "quarry"
                    })),
            )
            .expect(1)
            .mount(&auth)
            .await;
        let provider =
            TokenProvider::new_for_test(&auth.uri(), "execution-core", "execution-core-secret");

        let requests = (0..16).map(|_| {
            let provider = provider.clone();
            tokio::spawn(
                async move { provider.token("org-a", &["browser:execute"]).await.unwrap() },
            )
        });
        for request in requests {
            assert_eq!(request.await.unwrap(), "shared-token");
        }
    }

    #[tokio::test]
    async fn conditional_invalidation_preserves_a_peer_replacement() {
        let auth = MockServer::start().await;
        let mint_count = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with({
                let mint_count = mint_count.clone();
                move |_: &wiremock::Request| {
                    let sequence = mint_count.fetch_add(1, Ordering::SeqCst);
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "token": format!("minted-token-{sequence}"),
                        "expiresInSeconds": 300,
                        "audience": "quarry"
                    }))
                }
            })
            .expect(2)
            .mount(&auth)
            .await;
        let provider = TokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");

        let rejected = provider.token("org-a", &["browser:execute"]).await.unwrap();
        provider
            .invalidate_if_matches("org-a", &["browser:execute"], &rejected)
            .await
            .unwrap();
        let replacement = provider.token("org-a", &["browser:execute"]).await.unwrap();
        provider
            .invalidate_if_matches("org-a", &["browser:execute"], &rejected)
            .await
            .unwrap();

        assert_eq!(
            provider.token("org-a", &["browser:execute"]).await.unwrap(),
            replacement
        );
    }

    #[tokio::test]
    async fn concurrent_unauthorized_refreshes_mint_one_replacement() {
        let auth = MockServer::start().await;
        let mint_count = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with({
                let mint_count = mint_count.clone();
                move |_: &wiremock::Request| {
                    let sequence = mint_count.fetch_add(1, Ordering::SeqCst);
                    ResponseTemplate::new(200)
                        .set_delay(Duration::from_millis(25))
                        .set_body_json(serde_json::json!({
                            "token": format!("minted-token-{sequence}"),
                            "expiresInSeconds": 300,
                            "audience": "quarry"
                        }))
                }
            })
            .expect(2)
            .mount(&auth)
            .await;
        let provider = TokenProvider::new_for_test(&auth.uri(), "execution-core", "secret");
        let rejected = provider.token("org-a", &["browser:execute"]).await.unwrap();

        let refreshes = (0..16).map(|_| {
            let provider = provider.clone();
            let rejected = rejected.clone();
            tokio::spawn(async move {
                provider
                    .invalidate_if_matches("org-a", &["browser:execute"], &rejected)
                    .await
                    .unwrap();
                provider.token("org-a", &["browser:execute"]).await.unwrap()
            })
        });
        for refresh in refreshes {
            assert_eq!(refresh.await.unwrap(), "minted-token-1");
        }
    }

    #[tokio::test]
    async fn cache_evicts_entries_at_its_configured_bound() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "bounded-token",
                "expiresInSeconds": 300,
                "audience": "quarry"
            })))
            .expect(5)
            .mount(&auth)
            .await;
        let provider = TokenProvider::new_for_test_with_cache_limit(
            &auth.uri(),
            "execution-core",
            "secret",
            4,
        );

        for index in 0..5 {
            provider
                .token(&format!("org-{index}"), &["browser:execute"])
                .await
                .unwrap();
        }

        assert_eq!(provider.cached_entry_count().await, 4);
    }
}
