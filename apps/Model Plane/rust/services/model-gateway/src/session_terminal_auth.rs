//! Fixed-scope service credentials for the managed-run terminalization API.
//!
//! This deliberately does not reuse the Quarry token provider: the Auth Core
//! audience, endpoint, reason, and scopes must remain exact. A caller can ask
//! only for a terminal-outcome or heartbeat token for its already-bound org.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::Mutex;

const SESSION_CORE_AUDIENCE: &str = "session-core";
const TERMINALIZE_SCOPE: &str = "session:terminalize";
const HEARTBEAT_SCOPE: &str = "session:heartbeat";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionTerminalTokenError {
    #[error("Session Core terminalizer service authentication is not configured: {0}")]
    Configuration(&'static str),
    #[error("Session Core terminalizer requires an organization")]
    MissingOrganization,
    #[error("Auth Core terminalizer token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the terminalizer credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid Session Core terminalizer token")]
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TerminalScope {
    Terminalize,
    Heartbeat,
}

impl TerminalScope {
    const fn scope(self) -> &'static str {
        match self {
            Self::Terminalize => TERMINALIZE_SCOPE,
            Self::Heartbeat => HEARTBEAT_SCOPE,
        }
    }

    const fn reason(self) -> &'static str {
        match self {
            Self::Terminalize => "model-gateway managed-run terminalization",
            Self::Heartbeat => "model-gateway managed-run heartbeat",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    org_id: String,
    scope: TerminalScope,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

struct ProviderState {
    cache: Mutex<HashMap<CacheKey, CachedToken>>,
    in_flight: DashMap<CacheKey, Arc<Mutex<()>>>,
}

/// A narrow minting client for Session Core's service-only terminal APIs.
///
/// The long-lived credential is never logged or exposed through `Debug`; Auth
/// Core exchanges it for a short-lived org-scoped `aud=session-core` JWT.
#[derive(Clone)]
pub(crate) struct SessionTerminalTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    state: Arc<ProviderState>,
}

impl fmt::Debug for SessionTerminalTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionTerminalTokenProvider")
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

impl SessionTerminalTokenProvider {
    pub(crate) fn from_env() -> Result<Self, SessionTerminalTokenError> {
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
    ) -> Result<Self, SessionTerminalTokenError> {
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
    fn new_for_test(auth_core_url: String, service_id: &str, credential: &str) -> Self {
        Self::from_service_credential(&auth_core_url, service_id.to_owned(), credential.to_owned())
            .expect("test Session Core terminalizer client")
    }

    pub(crate) async fn terminalize_token(
        &self,
        org_id: &str,
    ) -> Result<String, SessionTerminalTokenError> {
        self.token(org_id, TerminalScope::Terminalize).await
    }

    pub(crate) async fn heartbeat_token(
        &self,
        org_id: &str,
    ) -> Result<String, SessionTerminalTokenError> {
        self.token(org_id, TerminalScope::Heartbeat).await
    }

    async fn token(
        &self,
        org_id: &str,
        scope: TerminalScope,
    ) -> Result<String, SessionTerminalTokenError> {
        let key = cache_key(org_id, scope)?;
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

    async fn mint(&self, key: &CacheKey) -> Result<TokenResponse, SessionTerminalTokenError> {
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                SessionTerminalTokenError::Configuration("MODEL_GATEWAY_SERVICE_API_KEY")
            })?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{SESSION_CORE_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": &key.org_id,
                "scopes": [key.scope.scope()],
                "reason": key.scope.reason(),
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(SessionTerminalTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| SessionTerminalTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != SESSION_CORE_AUDIENCE
        {
            return Err(SessionTerminalTokenError::InvalidResponse);
        }
        Ok(bundle)
    }

    fn remove_unused_mint_lock(&self, key: &CacheKey, expected: &Arc<Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.state
                .in_flight
                .remove_if(key, |_, current| Arc::ptr_eq(current, expected));
        }
    }
}

fn cache_key(org_id: &str, scope: TerminalScope) -> Result<CacheKey, SessionTerminalTokenError> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err(SessionTerminalTokenError::MissingOrganization);
    }
    Ok(CacheKey {
        org_id: org_id.to_owned(),
        scope,
    })
}

fn required_env(name: &'static str) -> Result<String, SessionTerminalTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(SessionTerminalTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::SessionTerminalTokenProvider;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_only_the_fixed_session_terminal_and_heartbeat_scopes() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .and(header("x-service-id", "model-gateway"))
            .and(header("x-service-api-key", "gateway-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["session:terminalize"],
                "reason": "model-gateway managed-run terminalization"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "terminal-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .and(header("x-service-id", "model-gateway"))
            .and(header("x-service-api-key", "gateway-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["session:heartbeat"],
                "reason": "model-gateway managed-run heartbeat"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "heartbeat-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider = SessionTerminalTokenProvider::new_for_test(
            auth.uri(),
            "model-gateway",
            "gateway-secret",
        );

        assert_eq!(
            provider.terminalize_token("org-a").await.unwrap(),
            "terminal-token"
        );
        assert_eq!(
            provider.terminalize_token("org-a").await.unwrap(),
            "terminal-token",
            "the terminal token is cached only for the exact fixed scope"
        );
        assert_eq!(
            provider.heartbeat_token("org-a").await.unwrap(),
            "heartbeat-token"
        );
    }

    #[tokio::test]
    async fn rejects_a_non_session_core_token_response() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-audience-token",
                "expiresInSeconds": 300,
                "audience": "quarry"
            })))
            .mount(&auth)
            .await;
        let provider =
            SessionTerminalTokenProvider::new_for_test(auth.uri(), "model-gateway", "secret");

        assert!(provider.terminalize_token("org-a").await.is_err());
    }
}
