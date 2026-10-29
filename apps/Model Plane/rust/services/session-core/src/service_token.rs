//! Minting of Auth Core service tokens for session-core's outbound calls.
//!
//! session-core is a callee for almost everything it does, so the few outbound
//! calls it makes each need their own credential: one exact audience, one exact
//! scope, one tenant. This module is that machinery, factored out of the Letta
//! adapter that first needed it so the second caller (Dreaming's extraction
//! inference) does not carry a second copy of the stampede lock, the TTL
//! validation, the response size caps and the cache eviction.
//!
//! # What it deliberately refuses to do
//!
//! **It never widens.** A provider is constructed with the exact audience and
//! the exact scopes it may ever ask for, and a request for anything outside
//! that list is a configuration error rather than a mint attempt. Auth Core's
//! registry is a ceiling, not a request — a caller that asks for less than it
//! was granted is the point.
//!
//! **It never fabricates.** Auth Core's answer is checked (audience matches,
//! token non-empty and whitespace-free, TTL sane) before it is cached. A
//! malformed answer fails closed rather than becoming a credential.
//!
//! **It never mints once per fleet.** Tokens are cached per `(org, scope)`,
//! because Auth Core issues them per tenant and a token minted for one
//! organization must never be presented on another's behalf.

use serde::Deserialize;
use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const AUTH_CORE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const AUTH_CORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Refresh this far before expiry so a token cannot expire mid-flight.
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
/// A service token that outlives an hour is not a service token.
const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;
const MAX_TOKEN_RESPONSE_BYTES: usize = 65_536;
/// Bounded so a tenant-per-key cache cannot become an unbounded leak in a
/// deployment with many organizations.
const MAX_TOKEN_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServiceTokenError {
    #[error("invalid service caller configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Auth Core token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the service caller credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid service caller credential")]
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TokenCacheKey {
    org_id: String,
    scope: String,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Mints and caches Auth Core tokens for exactly one audience.
#[derive(Clone)]
pub(crate) struct ServiceTokenProvider {
    auth_core_url: String,
    audience: &'static str,
    /// Every scope this provider may ever request. Anything else is refused
    /// before a request leaves the process.
    allowed_scopes: &'static [&'static str],
    /// Recorded in Auth Core's issuance audit, so a token in the log can be
    /// traced back to the subsystem that asked for it.
    reason: &'static str,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<TokenCacheKey, CachedToken>>>,
    mint_lock: Arc<Mutex<()>>,
}

impl fmt::Debug for ServiceTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("audience", &self.audience)
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
    audience: String,
}

impl ServiceTokenProvider {
    pub(crate) fn new(
        auth_core_url: &str,
        audience: &'static str,
        allowed_scopes: &'static [&'static str],
        reason: &'static str,
        service_id: &str,
        credential: &str,
    ) -> Result<Self, ServiceTokenError> {
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            audience,
            allowed_scopes,
            reason,
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(AUTH_CORE_CONNECT_TIMEOUT)
                .timeout(AUTH_CORE_REQUEST_TIMEOUT)
                .build()?,
            cache: Arc::new(Mutex::new(HashMap::new())),
            mint_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Return a valid bearer for `org_id` at exactly one scope.
    ///
    /// One scope rather than a set: a token carrying two scopes is presented on
    /// calls that only need one of them, and the narrower credential costs
    /// nothing but a cache entry.
    pub(crate) async fn token(
        &self,
        org_id: &str,
        scopes: &[&str],
    ) -> Result<String, ServiceTokenError> {
        let key = self.cache_key(org_id, scopes)?;
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + TOKEN_REFRESH_SKEW);
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.token.clone());
            }
        }

        // A miss is uncommon and bounded by the Auth Core TTL. Serialize misses
        // so concurrent callers cannot stampede the audited issuance endpoint;
        // re-check after taking the lock.
        let _mint_guard = self.mint_lock.lock().await;
        let now = Instant::now();
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache
                .get(&key)
                .filter(|cached| cached.expires_at > now + TOKEN_REFRESH_SKEW)
            {
                return Ok(cached.token.clone());
            }
        }
        let response = self.mint(&key).await?;
        let token = response.token;
        let mut cache = self.cache.lock().await;
        cache.retain(|_, cached| cached.expires_at > now + TOKEN_REFRESH_SKEW);
        if cache.len() >= MAX_TOKEN_CACHE_ENTRIES {
            if let Some(eviction_key) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.expires_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&eviction_key);
            }
        }
        cache.insert(
            key,
            CachedToken {
                token: token.clone(),
                expires_at: now + Duration::from_secs(response.expires_in_seconds),
            },
        );
        Ok(token)
    }

    fn cache_key(&self, org_id: &str, scopes: &[&str]) -> Result<TokenCacheKey, ServiceTokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() || org_id.chars().any(char::is_control) {
            return Err(ServiceTokenError::InvalidConfiguration(
                "service token organization",
            ));
        }
        let [scope] = scopes else {
            return Err(ServiceTokenError::InvalidConfiguration(
                "service token scope",
            ));
        };
        if !self.allowed_scopes.contains(scope) {
            return Err(ServiceTokenError::InvalidConfiguration(
                "service token scope",
            ));
        }
        Ok(TokenCacheKey {
            org_id: org_id.to_owned(),
            scope: (*scope).to_owned(),
        })
    }

    async fn mint(&self, key: &TokenCacheKey) -> Result<TokenResponse, ServiceTokenError> {
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| ServiceTokenError::InvalidConfiguration("service caller credential"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{}/internal-token",
                self.auth_core_url, self.audience
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": &key.org_id,
                "scopes": [&key.scope],
                "reason": self.reason,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ServiceTokenError::Refused(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
        {
            return Err(ServiceTokenError::InvalidResponse);
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(ServiceTokenError::InvalidResponse);
        }
        let bundle: TokenResponse =
            serde_json::from_slice(&bytes).map_err(|_| ServiceTokenError::InvalidResponse)?;
        // The audience check is the load-bearing one: a token for a different
        // plane presented here would be a credential this service was never
        // granted, and Auth Core echoing the wrong audience is exactly the
        // misconfiguration that must not become a working call.
        if bundle.audience != self.audience
            || bundle.token.trim().is_empty()
            || bundle.token.chars().any(char::is_whitespace)
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            return Err(ServiceTokenError::InvalidResponse);
        }
        Ok(bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SCOPES: &[&str] = &["inference:invoke"];

    fn provider(auth_core_url: &str) -> ServiceTokenProvider {
        ServiceTokenProvider::new(
            auth_core_url,
            "inference-core",
            SCOPES,
            "session-core dreaming extraction",
            "session-core",
            "session-core-secret",
        )
        .expect("provider")
    }

    #[tokio::test]
    async fn mints_the_exact_audience_tenant_and_scope_then_caches() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/inference-core/internal-token"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-api-key", "session-core-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["inference:invoke"],
                "reason": "session-core dreaming extraction"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "inference-token",
                "expiresInSeconds": 300,
                "audience": "inference-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider = provider(&auth.uri());
        for _ in 0..3 {
            assert_eq!(
                provider.token("org-a", SCOPES).await.unwrap(),
                "inference-token"
            );
        }
    }

    /// A token for another plane is a credential this service was never
    /// granted, however well-formed the response looks.
    #[tokio::test]
    async fn a_mismatched_audience_is_never_used() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/inference-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-plane-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .mount(&auth)
            .await;

        let error = provider(&auth.uri())
            .token("org-a", SCOPES)
            .await
            .expect_err("a wrong-audience token must fail closed");
        assert!(error.to_string().contains("invalid"));
    }

    /// Auth Core's registry is a ceiling. Asking for a scope the provider was
    /// not constructed with must not even reach the network — otherwise a
    /// widened registry entry would silently widen this caller too.
    #[tokio::test]
    async fn an_unlisted_scope_is_refused_before_any_request() {
        let auth = MockServer::start().await;
        // No mock is mounted on purpose: reaching the server at all is failure.
        let error = provider(&auth.uri())
            .token("org-a", &["inference:read"])
            .await
            .expect_err("an unlisted scope must be refused");
        assert!(error.to_string().contains("scope"));

        let error = provider(&auth.uri())
            .token("org-a", &["inference:invoke", "inference:read"])
            .await
            .expect_err("a multi-scope token must be refused");
        assert!(error.to_string().contains("scope"));
    }

    #[tokio::test]
    async fn a_blank_tenant_is_refused() {
        let auth = MockServer::start().await;
        let error = provider(&auth.uri())
            .token("   ", SCOPES)
            .await
            .expect_err("a blank tenant must be refused");
        assert!(error.to_string().contains("organization"));
    }

    /// Tokens are per tenant, so a second organization must mint its own rather
    /// than reuse the first one's cached credential.
    #[tokio::test]
    async fn each_tenant_gets_its_own_token() {
        let auth = MockServer::start().await;
        for (org, token) in [("org-a", "token-a"), ("org-b", "token-b")] {
            Mock::given(method("POST"))
                .and(path("/api/inference-core/internal-token"))
                .and(body_json(serde_json::json!({
                    "orgId": org,
                    "scopes": ["inference:invoke"],
                    "reason": "session-core dreaming extraction"
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "token": token,
                    "expiresInSeconds": 300,
                    "audience": "inference-core"
                })))
                .expect(1)
                .mount(&auth)
                .await;
        }

        let provider = provider(&auth.uri());
        assert_eq!(provider.token("org-a", SCOPES).await.unwrap(), "token-a");
        assert_eq!(provider.token("org-b", SCOPES).await.unwrap(), "token-b");
        assert_eq!(provider.token("org-a", SCOPES).await.unwrap(), "token-a");
    }

    /// An implausible TTL is a misconfigured issuer, not a long-lived token.
    #[tokio::test]
    async fn an_out_of_range_ttl_is_refused() {
        for ttl in [0u64, MAX_TOKEN_TTL_SECONDS + 1] {
            let auth = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api/inference-core/internal-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "token": "some-token",
                    "expiresInSeconds": ttl,
                    "audience": "inference-core"
                })))
                .mount(&auth)
                .await;
            let error = provider(&auth.uri())
                .token("org-a", SCOPES)
                .await
                .expect_err("an implausible TTL must fail closed");
            assert!(error.to_string().contains("invalid"));
        }
    }

    #[test]
    fn the_credential_never_appears_in_debug_output() {
        let rendered = format!("{:?}", provider("http://auth-core:3011"));
        assert!(!rendered.contains("session-core-secret"));
        assert!(rendered.contains("REDACTED"));
    }
}
