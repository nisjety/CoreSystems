//! Dedicated service credential for model-gateway's browser-broker revokes (W3-DEC-REVOKE, owner decision D16).
//!
//! A run-end revoke of a browser run grant used to present the ingress user bearer, which Auth Core mints for
//! about 300 seconds. A run that outlived that bearer had its revoke refused, so its grants ended only at their TTL.
//! This is model-gateway's own Auth Core principal, `model-gateway-browser-admin` in
//! `apps/Control Plane/config/plane-service-principals.json`: one audience (`browser-broker`), one scope
//! (`browser:write`), per organization, retention `zdr` because a revoke carries no content. Like
//! `mail_task_bridge_auth`, it is its own principal rather than a reuse of another one, so its retention posture
//! and scope stay its own.
//!
//! The provider mints an organization-scoped `aud=browser-broker` token and keeps it until 30 seconds before it
//! expires. It is optional: without `MODEL_GATEWAY_BROWSER_ADMIN_SERVICE_API_KEY`, or when Auth Core refuses the
//! mint, the revoke falls back to the ingress bearer. The credential is never logged and is excluded from `Debug`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

const BROWSER_BROKER_AUDIENCE: &str = "browser-broker";
const SCOPE: &str = "browser:write";
const REASON: &str = "model-gateway browser run revoke";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum BrowserAdminTokenError {
    #[error("browser-broker revoke credential is not configured: {0}")]
    Configuration(&'static str),
    #[error("browser-broker revoke token requires an organization")]
    MissingOrganization,
    #[error("Auth Core browser-broker revoke token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the browser-broker revoke credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid browser-broker revoke token")]
    InvalidResponse,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Mints short-lived, organization-scoped `browser:write` tokens for model-gateway's browser-broker principal, with
/// a small in-process cache keyed by organization.
#[derive(Clone)]
pub(crate) struct BrowserAdminTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, CachedToken>>>,
}

impl fmt::Debug for BrowserAdminTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserAdminTokenProvider")
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

impl BrowserAdminTokenProvider {
    pub(crate) fn from_env() -> Result<Self, BrowserAdminTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("MODEL_GATEWAY_BROWSER_ADMIN_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "model-gateway-browser-admin".to_owned());
        let credential = required_env("MODEL_GATEWAY_BROWSER_ADMIN_SERVICE_API_KEY")?;
        Self::new(&auth_core_url, service_id, credential)
    }

    pub(crate) fn new(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, BrowserAdminTokenError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// This organization's `browser:write` token, served from the cache or minted fresh.
    pub(crate) async fn token(&self, org_id: &str) -> Result<String, BrowserAdminTokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(BrowserAdminTokenError::MissingOrganization);
        }
        {
            let now = Instant::now();
            let mut cache = self.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
            if let Some(cached) = cache.get(org_id) {
                return Ok(cached.token.clone());
            }
        }

        let minted = self.mint(org_id).await?;
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        cache.retain(|_, cached| cached.expires_at > now + REFRESH_SKEW);
        if !cache.contains_key(org_id) && cache.len() >= MAX_CACHE_ENTRIES {
            if let Some(eviction_key) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.expires_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&eviction_key);
            }
        }
        cache.insert(
            org_id.to_owned(),
            CachedToken {
                token: minted.token.clone(),
                expires_at: now + Duration::from_secs(minted.expires_in_seconds),
            },
        );
        Ok(minted.token)
    }

    async fn mint(&self, org_id: &str) -> Result<TokenResponse, BrowserAdminTokenError> {
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
            BrowserAdminTokenError::Configuration("MODEL_GATEWAY_BROWSER_ADMIN_SERVICE_API_KEY")
        })?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{BROWSER_BROKER_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [SCOPE],
                "reason": REASON,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(BrowserAdminTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| BrowserAdminTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != BROWSER_BROKER_AUDIENCE
        {
            return Err(BrowserAdminTokenError::InvalidResponse);
        }
        Ok(bundle)
    }
}

fn required_env(name: &'static str) -> Result<String, BrowserAdminTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(BrowserAdminTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::{BrowserAdminTokenError, BrowserAdminTokenProvider};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SERVICE_ID: &str = "model-gateway-browser-admin";
    // A fake credential of repeated letters, so the secret scanner passes it.
    const CREDENTIAL: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const MINTED: &str = "minted-browser-broker-token";

    fn provider_for(auth_core_url: &str) -> BrowserAdminTokenProvider {
        BrowserAdminTokenProvider::new(auth_core_url, SERVICE_ID.to_owned(), CREDENTIAL.to_owned())
            .expect("the revoke token client builds")
    }

    fn minted(expires_in_seconds: u64, audience: &str, token: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": token,
            "expiresInSeconds": expires_in_seconds,
            "audience": audience,
        }))
    }

    #[tokio::test]
    async fn mints_an_org_scoped_browser_write_token_and_serves_the_second_call_from_cache() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/browser-broker/internal-token"))
            .and(header("x-service-id", SERVICE_ID))
            .and(header("x-service-api-key", CREDENTIAL))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["browser:write"],
                "reason": "model-gateway browser run revoke",
            })))
            .respond_with(minted(300, "browser-broker", MINTED))
            .expect(1)
            .mount(&auth)
            .await;

        let provider = provider_for(&auth.uri());
        assert_eq!(provider.token("org-a").await.expect("mint succeeds"), MINTED);
        // The mock allows exactly one POST, so this call is served from the cache.
        assert_eq!(provider.token(" org-a ").await.expect("cache hit"), MINTED);
    }

    #[tokio::test]
    async fn a_redirect_from_auth_core_is_refused_and_never_followed() {
        let auth = MockServer::start().await;
        let elsewhere = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/browser-broker/internal-token"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/elsewhere", elsewhere.uri())),
            )
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .respond_with(minted(300, "browser-broker", MINTED))
            .expect(0)
            .mount(&elsewhere)
            .await;

        let error = provider_for(&auth.uri())
            .token("org-a")
            .await
            .expect_err("a redirect is not a token");
        assert!(matches!(
            error,
            BrowserAdminTokenError::Refused(status)
                if status == reqwest::StatusCode::TEMPORARY_REDIRECT
        ));
    }

    #[tokio::test]
    async fn a_token_for_another_audience_or_an_out_of_range_ttl_or_an_empty_token_is_invalid() {
        let cases = [
            ("session-core", 300, MINTED),
            ("", 300, MINTED),
            ("browser-broker", 0, MINTED),
            ("browser-broker", 3601, MINTED),
            ("browser-broker", 300, "   "),
        ];
        for (audience, ttl, token) in cases {
            let auth = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api/browser-broker/internal-token"))
                .respond_with(minted(ttl, audience, token))
                .mount(&auth)
                .await;
            let error = provider_for(&auth.uri())
                .token("org-a")
                .await
                .expect_err("an unusable token is refused");
            assert!(
                matches!(error, BrowserAdminTokenError::InvalidResponse),
                "audience {audience:?}, ttl {ttl}, token {token:?}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn an_empty_organization_is_refused_before_any_mint() {
        assert!(matches!(
            provider_for("http://unused.invalid").token("   ").await,
            Err(BrowserAdminTokenError::MissingOrganization)
        ));
    }
}
