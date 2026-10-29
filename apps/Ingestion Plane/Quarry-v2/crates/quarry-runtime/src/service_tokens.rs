//! Short-lived, tenant-scoped service bearer acquisition from Auth Core.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

const SERVICE_ID: &str = "quarry-edge";
const DEFAULT_REFRESH_SKEW: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Clone)]
pub struct ServiceBearer(Arc<str>);

impl ServiceBearer {
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn from_test_token(token: impl Into<String>) -> Self {
        Self(Arc::from(token.into()))
    }
}

impl fmt::Debug for ServiceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceBearer(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceTokenAudience {
    DataPlane,
    ModelPlane,
}

impl ServiceTokenAudience {
    fn endpoint(self) -> &'static str {
        match self {
            Self::DataPlane => "/api/data-plane/internal-token",
            Self::ModelPlane => "/api/model-plane/internal-token",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceTokenRequest {
    audience: ServiceTokenAudience,
    scopes: Vec<String>,
    reason: String,
}

impl ServiceTokenRequest {
    pub fn data_plane<I, S>(scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(ServiceTokenAudience::DataPlane, scopes, reason)
    }

    pub fn model_plane<I, S>(scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(ServiceTokenAudience::ModelPlane, scopes, reason)
    }

    fn new<I, S>(audience: ServiceTokenAudience, scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut scopes: Vec<String> = scopes
            .into_iter()
            .map(Into::into)
            .map(|scope| scope.trim().to_owned())
            .filter(|scope| !scope.is_empty())
            .collect();
        scopes.sort();
        scopes.dedup();
        Self {
            audience,
            scopes,
            reason: reason.into().trim().to_owned(),
        }
    }

    fn validate(&self) -> QuarryResult<()> {
        if self.scopes.is_empty() || !(3..=500).contains(&self.reason.len()) {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "invalid bounded service-token request",
            ));
        }
        Ok(())
    }
}

#[async_trait]
pub trait ServiceTokenProvider: Send + Sync {
    async fn token_for_org(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<ServiceBearer>;

    async fn invalidate(&self, org_id: &str, request: &ServiceTokenRequest);
}

pub type SharedServiceTokenProvider = Arc<dyn ServiceTokenProvider>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    audience: ServiceTokenAudience,
    org_id: String,
    scopes: Vec<String>,
}

struct CachedToken {
    bearer: ServiceBearer,
    refresh_at: Instant,
}

pub struct AuthCoreServiceTokenProvider {
    http: Client,
    auth_core_url: String,
    service_api_key: Arc<str>,
    refresh_skew: Duration,
    cache: tokio::sync::RwLock<HashMap<CacheKey, CachedToken>>,
    in_flight: dashmap::DashMap<CacheKey, Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MintRequest<'a> {
    org_id: &'a str,
    scopes: &'a [String],
    reason: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MintResponse {
    token: String,
    expires_in_seconds: u64,
}

impl AuthCoreServiceTokenProvider {
    pub fn new(
        auth_core_url: impl Into<String>,
        service_api_key: impl Into<String>,
    ) -> QuarryResult<Self> {
        let auth_core_url = auth_core_url.into().trim().trim_end_matches('/').to_owned();
        let service_api_key = service_api_key.into();
        if auth_core_url.is_empty() || reqwest::Url::parse(&auth_core_url).is_err() {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "AUTH_CORE_URL must be an absolute URL",
            ));
        }
        if service_api_key.len() < 16 {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "QUARRY_SERVICE_API_KEY must contain at least 16 characters",
            ));
        }
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // A redirect could move the durable service credential to a
            // different origin. Auth Core endpoints are canonical and must
            // return their token response directly.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("Auth Core token client initialization failed: {error}"),
                )
            })?;
        Ok(Self {
            http,
            auth_core_url,
            service_api_key: Arc::from(service_api_key),
            refresh_skew: DEFAULT_REFRESH_SKEW,
            cache: tokio::sync::RwLock::new(HashMap::new()),
            in_flight: dashmap::DashMap::new(),
        })
    }

    fn cache_key(org_id: &str, request: &ServiceTokenRequest) -> CacheKey {
        CacheKey {
            audience: request.audience,
            org_id: org_id.to_owned(),
            scopes: request.scopes.clone(),
        }
    }

    async fn mint(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
    ) -> QuarryResult<MintResponse> {
        let url = format!("{}{}", self.auth_core_url, request.audience.endpoint());
        let response = self
            .http
            .post(url)
            .header("x-service-id", SERVICE_ID)
            .header("x-service-api-key", self.service_api_key.as_ref())
            .json(&MintRequest {
                org_id,
                scopes: &request.scopes,
                reason: &request.reason,
            })
            .send()
            .await
            .map_err(|error| {
                let code = if error.is_timeout() {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::UpstreamBlocked
                };
                QuarryError::new(code, "Auth Core service-token request failed")
            })?;
        let status = response.status();
        if !status.is_success() {
            let code = match status.as_u16() {
                401 | 403 => ErrorCode::Forbidden,
                429 => ErrorCode::RateLimited,
                _ => ErrorCode::DriverFailed,
            };
            return Err(QuarryError::new(
                code,
                format!("Auth Core service-token request returned {status}"),
            ));
        }
        let minted = response.json::<MintResponse>().await.map_err(|_| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "Auth Core returned an invalid service-token response",
            )
        })?;
        if minted.token.trim().is_empty() || minted.expires_in_seconds == 0 {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "Auth Core returned an unusable service-token response",
            ));
        }
        Ok(minted)
    }
}

#[async_trait]
impl ServiceTokenProvider for AuthCoreServiceTokenProvider {
    async fn token_for_org(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<ServiceBearer> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "verified org_id is required for cross-plane authentication",
            ));
        }
        request.validate()?;
        let key = Self::cache_key(org_id, request);
        if !force_refresh {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if Instant::now() < cached.refresh_at {
                    return Ok(cached.bearer.clone());
                }
            }
        }

        // Singleflight is per tenant/audience/scope key. A slow mint for one
        // organization never blocks cached reads or unrelated tenants.
        let mint_lock = self
            .in_flight
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let mint_guard = mint_lock.lock().await;
        if !force_refresh {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if Instant::now() < cached.refresh_at {
                    drop(mint_guard);
                    self.remove_unused_mint_lock(&key, &mint_lock);
                    return Ok(cached.bearer.clone());
                }
            }
        }
        let minted = match self.mint(org_id, request).await {
            Ok(minted) => minted,
            Err(error) => {
                drop(mint_guard);
                self.remove_unused_mint_lock(&key, &mint_lock);
                return Err(error);
            }
        };
        let refresh_after =
            Duration::from_secs(minted.expires_in_seconds).saturating_sub(self.refresh_skew);
        let bearer = ServiceBearer(Arc::from(minted.token));
        let mut cache = self.cache.write().await;
        cache.retain(|_, cached| Instant::now() < cached.refresh_at);
        if cache.len() >= MAX_CACHE_ENTRIES {
            let to_drop: Vec<CacheKey> =
                cache.keys().take(MAX_CACHE_ENTRIES / 4).cloned().collect();
            for expired_key in to_drop {
                cache.remove(&expired_key);
            }
        }
        cache.insert(
            key,
            CachedToken {
                bearer: bearer.clone(),
                refresh_at: Instant::now() + refresh_after,
            },
        );
        drop(cache);
        drop(mint_guard);
        self.remove_unused_mint_lock(&Self::cache_key(org_id, request), &mint_lock);
        Ok(bearer)
    }

    async fn invalidate(&self, org_id: &str, request: &ServiceTokenRequest) {
        self.cache
            .write()
            .await
            .remove(&Self::cache_key(org_id.trim(), request));
    }
}

impl AuthCoreServiceTokenProvider {
    fn remove_unused_mint_lock(&self, key: &CacheKey, expected: &Arc<tokio::sync::Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.in_flight
                .remove_if(key, |_, current| Arc::ptr_eq(current, expected));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_and_caches_an_org_scoped_data_plane_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/data-plane/internal-token"))
            .and(header("x-service-id", "quarry-edge"))
            .and(header("x-service-api-key", "credential-long-enough"))
            .and(body_json(json!({
                "orgId": "org_verified",
                "scopes": ["documents:write"],
                "reason": "persist verified Quarry evidence"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "header.payload.signature",
                "expiresAt": "2099-01-01T00:00:00.000Z",
                "expiresInSeconds": 300,
                "issuer": "auth-core",
                "audience": "data-plane"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        let first = provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();
        let second = provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();

        assert_eq!(first.expose(), "header.payload.signature");
        assert_eq!(second.expose(), first.expose());
        assert_eq!(format!("{first:?}"), "ServiceBearer(<redacted>)");
    }

    #[tokio::test]
    async fn forced_refresh_mints_a_replacement_model_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-plane/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "header.payload.signature",
                "expiresInSeconds": 300
            })))
            .expect(2)
            .mount(&server)
            .await;
        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request =
            ServiceTokenRequest::model_plane(["models:invoke"], "invoke bounded model primitive");

        provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();
        provider
            .token_for_org("org_verified", &request, true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn refuses_unverified_empty_org_before_network_io() {
        let server = MockServer::start().await;
        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        let error = provider
            .token_for_org("  ", &request, false)
            .await
            .unwrap_err();
        assert_eq!(error.code, quarry_core::error::ErrorCode::Forbidden);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn never_forwards_service_credentials_across_redirects() {
        let credential_sink = MockServer::start().await;
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/data-plane/internal-token"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/stolen", credential_sink.uri())),
            )
            .mount(&auth_core)
            .await;
        let provider =
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        assert!(provider
            .token_for_org("org_verified", &request, false)
            .await
            .is_err());
        assert!(credential_sink
            .received_requests()
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_short_service_credentials() {
        let result = AuthCoreServiceTokenProvider::new("http://auth-core:3011", "too-short");
        assert!(result.is_err());
    }
}
