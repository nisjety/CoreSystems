use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{DecodingKey, jwk::JwkSet};
use tokio::sync::RwLock;

use crate::AuthCtxError;

/// In-process JWKS cache. Holds the parsed `JwkSet` plus the time it
/// was fetched; refreshes on cache miss when the entry is older than
/// `ttl`. Tokio `RwLock` lets multiple readers share the cache without
/// blocking on the refresh path.
#[derive(Debug)]
pub struct JwksCache {
    url: String,
    ttl: Duration,
    state: Arc<RwLock<CacheState>>,
    client: reqwest::Client,
}

#[derive(Debug, Default)]
struct CacheState {
    fetched_at: Option<Instant>,
    keyset: Option<JwkSet>,
}

impl JwksCache {
    /// Build a cache with the supplied JWKS URL + TTL. The HTTP client
    /// has a 5-second connect timeout to fail fast when auth-core is
    /// unreachable — the resulting `JwksUnavailable` is the signal that
    /// triggers enforce-mode 503 responses.
    ///
    /// # Panics
    ///
    /// Panics if the `reqwest` client builder fails — which is
    /// effectively unreachable with the default features enabled (no
    /// TLS backend negotiation can fail at construction time).
    pub fn new(url: impl Into<String>, ttl: Duration) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client builder is infallible with default features");
        Self {
            url: url.into(),
            ttl,
            state: Arc::new(RwLock::new(CacheState::default())),
            client,
        }
    }

    /// Find a JWK matching the supplied key id, refreshing the JWKS on
    /// cache miss or stale entry. Returns `Ok(None)` when the JWKS is
    /// fresh but does not contain `kid` — the caller surfaces this as
    /// a signature-verification error so an attacker can't smuggle in
    /// an unknown kid.
    ///
    /// # Errors
    ///
    /// Returns [`AuthCtxError::JwksUnavailable`] when the JWKS endpoint
    /// is unreachable, returns non-2xx, or serves a body that doesn't
    /// parse as a JWK set.
    pub async fn decoding_key(&self, kid: &str) -> Result<Option<DecodingKey>, AuthCtxError> {
        if let Some(key) = self.lookup_fresh(kid).await {
            return Ok(Some(key));
        }
        self.refresh().await?;
        Ok(self.lookup_any(kid).await)
    }

    async fn lookup_fresh(&self, kid: &str) -> Option<DecodingKey> {
        let state = self.state.read().await;
        if let (Some(fetched_at), Some(keyset)) = (state.fetched_at, state.keyset.as_ref()) {
            if fetched_at.elapsed() < self.ttl {
                if let Some(jwk) = keyset.find(kid) {
                    return DecodingKey::from_jwk(jwk).ok();
                }
            }
        }
        None
    }

    async fn lookup_any(&self, kid: &str) -> Option<DecodingKey> {
        let state = self.state.read().await;
        state
            .keyset
            .as_ref()
            .and_then(|keyset| keyset.find(kid))
            .and_then(|jwk| DecodingKey::from_jwk(jwk).ok())
    }

    async fn refresh(&self) -> Result<(), AuthCtxError> {
        let res = self
            .client
            .get(&self.url)
            .send()
            .await
            .map_err(|e| AuthCtxError::JwksUnavailable(e.to_string()))?;
        if !res.status().is_success() {
            return Err(AuthCtxError::JwksUnavailable(format!(
                "JWKS endpoint returned {}",
                res.status()
            )));
        }
        let keyset: JwkSet = res
            .json()
            .await
            .map_err(|e| AuthCtxError::JwksUnavailable(format!("invalid JWKS JSON: {e}")))?;

        let mut state = self.state.write().await;
        state.fetched_at = Some(Instant::now());
        state.keyset = Some(keyset);
        Ok(())
    }
}
