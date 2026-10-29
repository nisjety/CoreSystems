//! Dedicated service credential for the finetune poller's session-core calls.
//!
//! Deliberately its own Auth Core principal, not a reuse of
//! `SessionTerminalTokenProvider`'s `model-gateway` credential: that one's
//! registry entry is `retentionByAudience.session-core = "zdr"`, correct for
//! its terminalize/heartbeat purpose (no durable content written). The
//! finetune poller's `UpdateJobStatus` calls persist real durable state (cost,
//! model id, deployment name), so they need a principal registered with
//! `retentionByAudience.session-core = "persistent"` instead — and that
//! posture is keyed per-principal in Auth Core's registry, not per-scope, so
//! it cannot be layered onto the existing `model-gateway` entry without also
//! (incorrectly) flipping the retention posture of its terminalize/heartbeat
//! tokens.
//!
//! Only one (org label, scope) pair is ever requested, so — unlike
//! `session_terminal_auth.rs`, which serves many concurrent per-org callers —
//! this needs no multi-key cache or in-flight-mint dedup: the poller is a
//! single sequential background loop, so one cached token behind one mutex is
//! sufficient and race-free by construction.

use std::fmt;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

const SESSION_CORE_AUDIENCE: &str = "session-core";
const SCOPE: &str = "session:finetune:poll";
const REASON: &str = "model-gateway finetune job poller";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3600;

#[derive(Debug, thiserror::Error)]
pub(crate) enum FinetunePollerTokenError {
    #[error("finetune poller session-core credential is not configured: {0}")]
    Configuration(&'static str),
    #[error("Auth Core finetune-poller token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the finetune-poller credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid finetune-poller token")]
    InvalidResponse,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// A narrow minting client for the finetune poller's one fixed
/// (audience, scope) pair. The long-lived credential is never logged or
/// exposed through `Debug`; Auth Core exchanges it for a short-lived
/// `aud=session-core` JWT.
pub(crate) struct FinetunePollerTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Mutex<Option<CachedToken>>,
}

impl fmt::Debug for FinetunePollerTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FinetunePollerTokenProvider")
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

impl FinetunePollerTokenProvider {
    pub(crate) fn from_env() -> Result<Self, FinetunePollerTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("FINETUNE_POLLER_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "model-gateway-finetune-poller".to_owned());
        let credential = required_env("FINETUNE_POLLER_SERVICE_API_KEY")?;
        Self::from_service_credential(&auth_core_url, service_id, credential)
    }

    pub(crate) fn from_service_credential(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, FinetunePollerTokenError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http,
            cache: Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self::from_service_credential(auth_core_url, service_id.to_owned(), credential.to_owned())
            .expect("test finetune-poller client")
    }

    /// The poller's session-core bearer, minted fresh or served from cache.
    /// Holds the cache mutex across the mint `.await` — safe and sufficient
    /// here because the poller has exactly one sequential caller; see the
    /// module doc for why this needs no in-flight-mint dedup unlike
    /// `session_terminal_auth.rs`'s multi-caller cache.
    pub(crate) async fn token(&self) -> Result<String, FinetunePollerTokenError> {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref() {
            if cached.expires_at > now + REFRESH_SKEW {
                return Ok(cached.token.clone());
            }
        }
        let minted = self.mint().await?;
        let token = minted.token.clone();
        *cache = Some(CachedToken {
            token: minted.token,
            expires_at: now + Duration::from_secs(minted.expires_in_seconds),
        });
        Ok(token)
    }

    async fn mint(&self) -> Result<TokenResponse, FinetunePollerTokenError> {
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                FinetunePollerTokenError::Configuration("FINETUNE_POLLER_SERVICE_API_KEY")
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
                "orgId": crate::finetune_poller::FINETUNE_POLLER_SERVICE_ORG_LABEL,
                "scopes": [SCOPE],
                "reason": REASON,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(FinetunePollerTokenError::Refused(response.status()));
        }
        let bundle = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| FinetunePollerTokenError::InvalidResponse)?;
        if bundle.token.trim().is_empty()
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
            || bundle.audience != SESSION_CORE_AUDIENCE
        {
            return Err(FinetunePollerTokenError::InvalidResponse);
        }
        Ok(bundle)
    }
}

fn required_env(name: &'static str) -> Result<String, FinetunePollerTokenError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(FinetunePollerTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_the_fixed_finetune_poll_scope_and_caches() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/internal-token"))
            .and(header("x-service-id", "model-gateway-finetune-poller"))
            .and(header("x-service-api-key", "poller-secret"))
            .and(body_json(serde_json::json!({
                "orgId": crate::finetune_poller::FINETUNE_POLLER_SERVICE_ORG_LABEL,
                "scopes": ["session:finetune:poll"],
                "reason": "model-gateway finetune job poller"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "poller-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider = FinetunePollerTokenProvider::new_for_test(
            &auth.uri(),
            "model-gateway-finetune-poller",
            "poller-secret",
        );

        assert_eq!(provider.token().await.unwrap(), "poller-token");
        assert_eq!(
            provider.token().await.unwrap(),
            "poller-token",
            "second call must be served from cache, not a second mint"
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
        let provider = FinetunePollerTokenProvider::new_for_test(
            &auth.uri(),
            "model-gateway-finetune-poller",
            "secret",
        );

        assert!(provider.token().await.is_err());
    }
}
