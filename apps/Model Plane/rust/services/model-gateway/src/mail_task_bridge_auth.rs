//! Dedicated service credential for the mail-task internal bridge's
//! session-core calls (delegated-identity design, Gap B §2.4(i)).
//!
//! Deliberately its OWN Auth Core principal, not a reuse of the existing
//! `model-gateway` principal `SessionTerminalTokenProvider` mints from: that
//! principal's registry entry is `retentionByAudience.session-core = "zdr"`
//! (correct for terminalize/heartbeat, which write no durable content) — the
//! SAME reasoning `finetune_poller_auth.rs` already documents for exactly
//! this situation. `PrepareUserOwnedThread` creates a real, durably-stored
//! thread, so it needs a principal registered with
//! `retentionByAudience.session-core = "persistent"` instead, and retention
//! posture is keyed per-principal (not per-scope) in Auth Core's registry —
//! it cannot be layered onto the existing entry without also (incorrectly)
//! flipping the retention posture of every OTHER credential that principal
//! already mints. See B-4/T-6 in the delegated-identity security review.
//!
//! Unlike the finetune poller (a single sequential background loop with one
//! fixed org label), this bridge is a multi-caller HTTP route serving real,
//! varying orgs — `PrepareUserOwnedThread`'s `authorize_org` requires the
//! credential's own `org_id` claim to exactly equal the request's org_id, so
//! this needs `session_terminal_auth.rs`'s per-org cache, not the poller's
//! single-slot one.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Deserialize;
use tokio::sync::Mutex;

const SESSION_CORE_AUDIENCE: &str = "session-core";
const SCOPE: &str = "session:user-owned-thread-prepare";
const REASON: &str = "model-gateway mail-task thread bridge";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum MailTaskBridgeTokenError {
    #[error("mail-task bridge session-core credential is not configured: {0}")]
    Configuration(&'static str),
    #[error("mail-task bridge requires an organization")]
    MissingOrganization,
    #[error("Auth Core mail-task bridge token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the mail-task bridge credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid mail-task bridge token")]
    InvalidResponse,
    /// B-4/T-6, made loud and specific instead of a downstream session-core
    /// `permission_denied` that gives an operator no clue what to fix: the
    /// minted token's own `zdr` claim was `true`, meaning THIS principal's
    /// `retentionByAudience.session-core` registry entry was never set to
    /// `"persistent"` (or was reverted) — `PrepareUserOwnedThread` fails
    /// closed for every mail_task-origin call until that is corrected.
    #[error(
        "mail-task bridge session-core credential is ZDR-scoped -- the \
         service:{0} registry row's retentionByAudience.session-core must be \
         \"persistent\", not \"zdr\" or absent, or every PrepareUserOwnedThread \
         call for mail_task origin fails closed (security-review B-4/T-6)"
    )]
    RetentionMisconfigured(String),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    org_id: String,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

struct ProviderState {
    cache: Mutex<HashMap<CacheKey, CachedToken>>,
    in_flight: DashMap<CacheKey, Arc<Mutex<()>>>,
}

/// A narrow minting client for the mail-task bridge's one fixed scope. The
/// long-lived credential is never logged or exposed through `Debug`; Auth
/// Core exchanges it for a short-lived, org-scoped `aud=session-core` JWT.
#[derive(Clone)]
pub(crate) struct MailTaskBridgeTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    state: Arc<ProviderState>,
}

impl fmt::Debug for MailTaskBridgeTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MailTaskBridgeTokenProvider")
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

impl MailTaskBridgeTokenProvider {
    pub(crate) fn from_env() -> Result<Self, MailTaskBridgeTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("MAIL_TASK_BRIDGE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "model-gateway-mail-task-bridge".to_owned());
        let credential = required_env("MAIL_TASK_BRIDGE_SERVICE_API_KEY")?;
        Self::from_service_credential(&auth_core_url, service_id, credential)
    }

    pub(crate) fn from_service_credential(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, MailTaskBridgeTokenError> {
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
            .expect("test mail-task bridge client")
    }

    /// This org's `session:user-owned-thread-prepare` bearer, minted fresh
    /// or served from cache. Fails with
    /// [`MailTaskBridgeTokenError::RetentionMisconfigured`] rather than
    /// silently handing back a ZDR token session-core will reject anyway.
    pub(crate) async fn token(&self, org_id: &str) -> Result<String, MailTaskBridgeTokenError> {
        let key = cache_key(org_id)?;
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

    async fn mint(&self, key: &CacheKey) -> Result<TokenResponse, MailTaskBridgeTokenError> {
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                MailTaskBridgeTokenError::Configuration("MAIL_TASK_BRIDGE_SERVICE_API_KEY")
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
                "scopes": [SCOPE],
                "reason": REASON,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MailTaskBridgeTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| MailTaskBridgeTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != SESSION_CORE_AUDIENCE
        {
            return Err(MailTaskBridgeTokenError::InvalidResponse);
        }
        if decoded_zdr_claim(&bundle.token).unwrap_or(true) {
            return Err(MailTaskBridgeTokenError::RetentionMisconfigured(
                self.service_id.clone(),
            ));
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

/// Reads the `zdr` claim out of the JWT payload segment without verifying
/// the signature — this is model-gateway's OWN just-received token from a
/// direct, trusted call to auth-core over the internal network, not an
/// untrusted input; the signature is verified downstream by session-core
/// the same way it verifies every other bearer. `None` (never trust-by-
/// default) on any decode failure, so a malformed token is treated as ZDR
/// (fails closed to `RetentionMisconfigured` via the caller's
/// `unwrap_or(true)`), not silently accepted.
fn decoded_zdr_claim(token: &str) -> Option<bool> {
    let payload = token.split('.').nth(1)?;
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value.get("zdr")?.as_bool()
}

fn cache_key(org_id: &str) -> Result<CacheKey, MailTaskBridgeTokenError> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err(MailTaskBridgeTokenError::MissingOrganization);
    }
    Ok(CacheKey {
        org_id: org_id.to_owned(),
    })
}

fn required_env(name: &'static str) -> Result<String, MailTaskBridgeTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(MailTaskBridgeTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::{MailTaskBridgeTokenError, MailTaskBridgeTokenProvider};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn jwt_with_zdr(zdr: bool) -> String {
        use base64::Engine as _;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({"alg":"RS256","typ":"JWT"}).to_string());
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({"zdr": zdr, "org_id": "org-a"}).to_string());
        format!("{header}.{payload}.test-signature")
    }

    #[tokio::test]
    async fn mints_the_fixed_scope_persistent_token_and_caches_per_org() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .and(header("x-service-id", "model-gateway-mail-task-bridge"))
            .and(header("x-service-api-key", "bridge-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["session:user-owned-thread-prepare"],
                "reason": "model-gateway mail-task thread bridge",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": jwt_with_zdr(false),
                "expiresInSeconds": 300,
                "audience": "session-core",
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider = MailTaskBridgeTokenProvider::new_for_test(
            auth.uri(),
            "model-gateway-mail-task-bridge",
            "bridge-secret",
        );

        let first = provider.token("org-a").await.expect("mint succeeds");
        let second = provider
            .token("org-a")
            .await
            .expect("second call served from cache");
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn fails_loudly_and_specifically_when_the_registry_still_sets_zdr_retention() {
        // security-review B-4/T-6: a ZDR-scoped minted token must be refused
        // with a clear, actionable error -- not handed to session-core to
        // fail closed with a generic, hard-to-diagnose permission_denied.
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": jwt_with_zdr(true),
                "expiresInSeconds": 300,
                "audience": "session-core",
            })))
            .mount(&auth)
            .await;

        let provider = MailTaskBridgeTokenProvider::new_for_test(
            auth.uri(),
            "model-gateway-mail-task-bridge",
            "bridge-secret",
        );

        let error = provider
            .token("org-a")
            .await
            .expect_err("a ZDR-scoped credential must be refused");
        assert!(matches!(
            error,
            MailTaskBridgeTokenError::RetentionMisconfigured(_)
        ));
        assert!(error.to_string().contains("persistent"));
    }

    #[tokio::test]
    async fn requires_an_organization() {
        let provider = MailTaskBridgeTokenProvider::new_for_test(
            "http://unused.invalid".to_owned(),
            "model-gateway-mail-task-bridge",
            "bridge-secret",
        );
        assert!(matches!(
            provider.token("   ").await,
            Err(MailTaskBridgeTokenError::MissingOrganization)
        ));
    }

    #[tokio::test]
    async fn rejects_a_non_session_core_token_response() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": jwt_with_zdr(false),
                "expiresInSeconds": 300,
                "audience": "quarry",
            })))
            .mount(&auth)
            .await;
        let provider = MailTaskBridgeTokenProvider::new_for_test(
            auth.uri(),
            "model-gateway-mail-task-bridge",
            "bridge-secret",
        );
        assert!(provider.token("org-a").await.is_err());
    }
}
