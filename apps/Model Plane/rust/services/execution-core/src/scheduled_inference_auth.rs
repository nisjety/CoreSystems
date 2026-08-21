//! Narrow Auth Core credential for the service-owned scheduled-step lane.
//!
//! Scheduled work must not borrow a human's delegated inference bearer.  This
//! provider mints only an `inference-core` token with `inference:invoke`,
//! scoped to the scheduled run's organization, and keeps the credential out of
//! requests, logs, and durable workflow state.

use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tokio::sync::Mutex;

const AUDIENCE: &str = "inference-core";
const SCOPE: &str = "inference:invoke";
const REASON: &str = "execution-core scheduled-step inference";
const REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TTL_SECONDS: u64 = 3_600;
const MAX_RESPONSE_BYTES: usize = 65_536;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ScheduledInferenceTokenError {
    #[error("scheduled inference service authentication is not configured: {0}")]
    Configuration(&'static str),
    #[error("scheduled inference token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the scheduled inference credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid scheduled inference credential")]
    InvalidResponse,
}

#[derive(Clone)]
pub(crate) struct ScheduledInferenceTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, CachedToken>>>,
    mint_lock: Arc<Mutex<()>>,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

impl fmt::Debug for ScheduledInferenceTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScheduledInferenceTokenProvider")
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
    audience: String,
}

impl ScheduledInferenceTokenProvider {
    pub(crate) fn from_env() -> Result<Self, ScheduledInferenceTokenError> {
        let auth_core_url = required_env("AUTH_CORE_URL")?;
        let service_id = std::env::var("EXECUTION_CORE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "execution-core".to_owned());
        let credential = required_env("EXECUTION_CORE_SERVICE_API_KEY")?;
        Self::new(&auth_core_url, service_id, credential)
    }

    fn new(
        auth_core_url: &str,
        service_id: String,
        credential: String,
    ) -> Result<Self, ScheduledInferenceTokenError> {
        Ok(Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            credential,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: Arc::new(Mutex::new(HashMap::new())),
            mint_lock: Arc::new(Mutex::new(())),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(auth_core_url: &str) -> Self {
        Self::new(
            auth_core_url,
            "execution-core".to_owned(),
            "dev-secret".to_owned(),
        )
        .expect("scheduled inference token provider")
    }

    pub(crate) async fn token(&self, org_id: &str) -> Result<String, ScheduledInferenceTokenError> {
        let org_id = org_id.trim();
        if org_id.is_empty() || org_id.chars().any(char::is_control) {
            return Err(ScheduledInferenceTokenError::Configuration(
                "scheduled inference organization",
            ));
        }
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, value| value.expires_at > now + REFRESH_SKEW);
            if let Some(value) = cache.get(org_id) {
                return Ok(value.token.clone());
            }
        }
        let _guard = self.mint_lock.lock().await;
        let now = Instant::now();
        {
            let cache = self.cache.lock().await;
            if let Some(value) = cache.get(org_id) {
                if value.expires_at > now + REFRESH_SKEW {
                    return Ok(value.token.clone());
                }
            }
        }
        let mut credential =
            reqwest::header::HeaderValue::from_str(&self.credential).map_err(|_| {
                ScheduledInferenceTokenError::Configuration("EXECUTION_CORE_SERVICE_API_KEY")
            })?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{AUDIENCE}/internal-token",
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
            return Err(ScheduledInferenceTokenError::Refused(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ScheduledInferenceTokenError::InvalidResponse);
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(ScheduledInferenceTokenError::InvalidResponse);
        }
        let bundle: TokenResponse = serde_json::from_slice(&bytes)
            .map_err(|_| ScheduledInferenceTokenError::InvalidResponse)?;
        if bundle.audience != AUDIENCE
            || bundle.token.trim().is_empty()
            || bundle.token.chars().any(char::is_whitespace)
            || !(1..=MAX_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            return Err(ScheduledInferenceTokenError::InvalidResponse);
        }
        let token = bundle.token;
        self.cache.lock().await.insert(
            org_id.to_owned(),
            CachedToken {
                token: token.clone(),
                expires_at: Instant::now() + Duration::from_secs(bundle.expires_in_seconds),
            },
        );
        Ok(token)
    }
}

fn required_env(name: &'static str) -> Result<String, ScheduledInferenceTokenError> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(ScheduledInferenceTokenError::Configuration(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_exact_inference_scope_and_caches_per_org() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/inference-core/internal-token"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["inference:invoke"],
                "reason": REASON,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "inference-token",
                "expiresInSeconds": 300,
                "audience": "inference-core",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = ScheduledInferenceTokenProvider::new_for_test(&server.uri());
        assert_eq!(provider.token("org-a").await.unwrap(), "inference-token");
        assert_eq!(provider.token("org-a").await.unwrap(), "inference-token");
    }

    #[tokio::test]
    async fn rejects_wrong_audience() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/inference-core/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong",
                "expiresInSeconds": 300,
                "audience": "session-core",
            })))
            .mount(&server)
            .await;
        let error = ScheduledInferenceTokenProvider::new_for_test(&server.uri())
            .token("org-a")
            .await
            .expect_err("wrong audience must fail closed");
        assert!(error.to_string().contains("invalid"));
    }
}
