//! Bearer tokens for Vertex AI, minted from a service-account key file.
//!
//! The key file is read once, when the Gemini provider registers. Each token is a
//! JWT signed with the key and exchanged at Google's token endpoint for a bearer
//! token that lasts an hour. The token is cached and replaced five minutes before
//! it expires, and concurrent callers queue on one exchange rather than each
//! starting their own.
//!
//! The key and the token never reach a log, an error message or `Debug` output.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::Instant;

use super::{provider_http_client, ProviderError};

/// The one scope a Vertex AI generation call needs.
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
/// Where the assertion is exchanged when the key file names no other endpoint.
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
/// The JWT bearer `grant_type`, already percent-encoded for the form body.
const JWT_BEARER_GRANT: &str = "urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer";
/// How long a signed assertion is valid. Google accepts at most one hour.
const ASSERTION_LIFETIME_SECS: u64 = 3_600;
/// A cached token is replaced this long before Google says it expires, so no call
/// starts with a token that can expire while it is in flight.
const REFRESH_MARGIN_SECS: u64 = 5 * 60;

/// The fields of a service-account key file the exchange needs. The rest of the
/// file is ignored, and nothing here is ever printed.
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    #[serde(default)]
    private_key_id: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    DEFAULT_TOKEN_URI.to_owned()
}

/// The claims of the assertion JWT.
#[derive(Serialize)]
struct Assertion<'a> {
    iss: &'a str,
    scope: &'static str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

/// Google's answer to a successful exchange.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// A bearer token and the instant after which it must be replaced.
struct CachedToken {
    value: String,
    expires_at: Instant,
}

/// Mints and caches the bearer token that every Vertex AI call of one service
/// account carries.
pub struct VertexTokenProvider {
    key: ServiceAccountKey,
    signing_key: EncodingKey,
    client: reqwest::Client,
    /// Held across the exchange on purpose: a caller that finds the cache stale
    /// waits here and reads the token the first caller stored.
    cached: Mutex<Option<CachedToken>>,
}

impl std::fmt::Debug for VertexTokenProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VertexTokenProvider")
            .field("client_email", &self.key.client_email)
            .field("key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl VertexTokenProvider {
    /// Read and check the key file at `path`. A missing, unreadable or malformed
    /// file is an error the caller turns into a refused registration. The error
    /// names neither the path nor anything the file contains.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] when the file cannot be read or is not
    /// a usable service-account key.
    pub fn from_file(path: &str) -> Result<Self, ProviderError> {
        let text = std::fs::read_to_string(path).map_err(|_| unreadable())?;
        Self::from_json(&text)
    }

    pub(crate) fn from_json(text: &str) -> Result<Self, ProviderError> {
        let key: ServiceAccountKey = serde_json::from_str(text).map_err(|_| not_a_key())?;
        if key.client_email.is_empty() || key.private_key.is_empty() {
            return Err(not_a_key());
        }
        // Parsing the PEM here makes a bad key refuse registration, instead of
        // failing every call later.
        let signing_key =
            EncodingKey::from_rsa_pem(key.private_key.as_bytes()).map_err(|_| not_a_key())?;
        Ok(Self {
            key,
            signing_key,
            client: provider_http_client(),
            cached: Mutex::new(None),
        })
    }

    /// A bearer token that stays valid for the refresh margin. A caller that finds
    /// the cache empty or stale holds the lock through the exchange, so concurrent
    /// callers share one exchange.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] when the exchange fails, and
    /// [`ProviderError::InvalidResponse`] when Google's answer does not parse.
    pub async fn access_token(&self) -> Result<String, ProviderError> {
        let mut cached = self.cached.lock().await;
        if let Some(token) = &*cached {
            if !needs_refresh(token.expires_at, Instant::now()) {
                return Ok(token.value.clone());
            }
        }
        let fresh = self.exchange().await?;
        let value = fresh.value.clone();
        *cached = Some(fresh);
        Ok(value)
    }

    async fn exchange(&self) -> Result<CachedToken, ProviderError> {
        let body = format!(
            "grant_type={JWT_BEARER_GRANT}&assertion={}",
            self.signed_assertion()?
        );
        let response = self
            .client
            .post(&self.key.token_uri)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await
            .map_err(|_| {
                ProviderError::Unavailable("vertex token exchange did not complete".to_owned())
            })?;
        if !response.status().is_success() {
            return Err(ProviderError::Unavailable(format!(
                "vertex token exchange refused: {}",
                response.status()
            )));
        }
        let token: TokenResponse = response.json().await.map_err(|_| {
            ProviderError::InvalidResponse("vertex token response did not parse".to_owned())
        })?;
        // Google's answer is outside input. Capping the lifetime at the one-hour maximum
        // keeps a malformed `expires_in` from overflowing the instant.
        let lifetime = token.expires_in.min(ASSERTION_LIFETIME_SECS);
        Ok(CachedToken {
            value: token.access_token,
            expires_at: Instant::now() + Duration::from_secs(lifetime),
        })
    }

    fn signed_assertion(&self) -> Result<String, ProviderError> {
        let issued = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let claims = Assertion {
            iss: &self.key.client_email,
            scope: CLOUD_PLATFORM_SCOPE,
            aud: &self.key.token_uri,
            iat: issued,
            exp: issued + ASSERTION_LIFETIME_SECS,
        };
        let mut header = Header::new(Algorithm::RS256);
        if !self.key.private_key_id.is_empty() {
            header.kid = Some(self.key.private_key_id.clone());
        }
        jsonwebtoken::encode(&header, &claims, &self.signing_key).map_err(|_| not_a_key())
    }
}

/// Whether a token that expires at `expires_at` must be replaced as of `now`.
fn needs_refresh(expires_at: Instant, now: Instant) -> bool {
    now + Duration::from_secs(REFRESH_MARGIN_SECS) >= expires_at
}

fn unreadable() -> ProviderError {
    ProviderError::Unavailable("vertex gemini credentials file is unreadable".to_owned())
}

fn not_a_key() -> ProviderError {
    ProviderError::Unavailable(
        "vertex gemini credentials file is not a service-account key".to_owned(),
    )
}

/// Fixtures shared with the Gemini provider's tests: one RSA key for the whole test
/// binary, since generating one per test is slow.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::OnceLock;

    use rand::thread_rng;
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};

    pub(crate) fn test_key_pem() -> &'static str {
        static PEM: OnceLock<String> = OnceLock::new();
        PEM.get_or_init(|| {
            let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
            private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("PKCS8 PEM")
                .to_string()
        })
    }

    /// A service-account key file body whose token exchange goes to `token_uri`.
    pub(crate) fn key_json(token_uri: &str) -> String {
        serde_json::json!({
            "type": "service_account",
            "project_id": "test-project",
            "private_key_id": "test-key-id",
            "private_key": test_key_pem(),
            "client_email": "gemini@test-project.iam.gserviceaccount.com",
            "token_uri": token_uri,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::test_support::{key_json, test_key_pem};
    use super::*;

    fn token_response(token: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": token,
            "expires_in": 3600,
            "token_type": "Bearer",
        }))
    }

    #[test]
    fn needs_refresh_only_inside_the_margin() {
        const TOKEN_LIFETIME_SECS: u64 = 3_600;
        let now = Instant::now();
        let expires = now + Duration::from_secs(TOKEN_LIFETIME_SECS);
        let boundary = TOKEN_LIFETIME_SECS - REFRESH_MARGIN_SECS;
        assert!(!needs_refresh(expires, now));
        assert!(!needs_refresh(
            expires,
            now + Duration::from_secs(boundary - 1)
        ));
        assert!(needs_refresh(expires, now + Duration::from_secs(boundary)));
    }

    #[tokio::test]
    async fn token_is_cached_until_refresh_margin() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("tok-one"))
            .expect(1)
            .mount(&server)
            .await;
        let provider =
            VertexTokenProvider::from_json(&key_json(&format!("{}/token", server.uri())))
                .expect("key");

        assert_eq!(provider.access_token().await.expect("first"), "tok-one");
        assert_eq!(provider.access_token().await.expect("second"), "tok-one");
        server.verify().await;
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("tok-shared").set_delay(Duration::from_millis(50)))
            .expect(1)
            .mount(&server)
            .await;
        let provider = Arc::new(
            VertexTokenProvider::from_json(&key_json(&format!("{}/token", server.uri())))
                .expect("key"),
        );

        let calls: Vec<_> = (0..8)
            .map(|_| {
                let provider = Arc::clone(&provider);
                tokio::spawn(async move { provider.access_token().await })
            })
            .collect();
        for call in futures_util::future::join_all(calls).await {
            assert_eq!(call.expect("join").expect("token"), "tok-shared");
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn debug_redacts_key_and_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("tok-secret-value"))
            .mount(&server)
            .await;
        let provider =
            VertexTokenProvider::from_json(&key_json(&format!("{}/token", server.uri())))
                .expect("key");
        provider.access_token().await.expect("token");

        let shown = format!("{provider:?}");
        assert!(shown.contains("REDACTED"), "{shown}");
        assert!(!shown.contains("tok-secret-value"), "{shown}");
        assert!(!shown.contains("PRIVATE KEY"), "{shown}");
        assert!(!shown.contains(test_key_pem().lines().nth(1).expect("PEM body")));
    }

    #[test]
    fn missing_file_refuses_registration_not_boot() {
        let missing = std::env::temp_dir().join("vertex-gemini-absent-key-file.json");
        let error = VertexTokenProvider::from_file(missing.to_str().expect("utf-8 path"))
            .expect_err("a missing file refuses registration");
        assert!(matches!(error, ProviderError::Unavailable(_)), "{error:?}");
        assert!(!error.to_string().contains("absent-key-file"), "{error}");
        assert!(VertexTokenProvider::from_json("{not json").is_err());
    }
}
