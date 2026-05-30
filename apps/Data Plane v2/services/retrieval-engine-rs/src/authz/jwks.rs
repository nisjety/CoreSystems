//! §16.5.5 — JWKS endpoint fetch with `kid` rotation support.
//!
//! Static `JWT_PUBLIC_KEY_PEM` is fine for closed deployments but breaks
//! down the moment auth-core rotates its signing key — we'd be stuck on a
//! redeploy to pick up the new key. This module fetches the JWKS document
//! once at boot and refreshes it on a background ticker, then resolves
//! incoming tokens by their `kid` header against the cached map.
//!
//! Env:
//!   - `JWT_JWKS_URL`        — HTTPS endpoint serving `{ keys: [...] }`. If
//!                             unset, JWKS is disabled and callers fall back
//!                             to `JWT_PUBLIC_KEY_PEM`.
//!   - `JWT_JWKS_REFRESH_SECS` — refresh interval (default 600).
//!
//! The fetcher is best-effort: if the refresh fails we keep serving the
//! previously-cached set. The first failed fetch at boot leaves the map
//! empty, which then forces fallback to the static PEM.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
struct JwksDoc {
    keys: Vec<JwksKey>,
}

#[derive(Debug, Deserialize)]
struct JwksKey {
    kid: String,
    #[serde(default)]
    alg: Option<String>,
    n: Option<String>, // RSA modulus (base64url)
    e: Option<String>, // RSA exponent (base64url)
}

#[derive(Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<HashMap<String, DecodingKey>>>,
}

static GLOBAL: OnceCell<JwksCache> = OnceCell::new();

impl JwksCache {
    /// Initialize the global JWKS cache. Idempotent; subsequent calls reuse
    /// the first instance. Returns `None` if `JWT_JWKS_URL` is not set —
    /// callers should then fall back to the static PEM path.
    pub fn init_global() -> Option<JwksCache> {
        let url = std::env::var("JWT_JWKS_URL")
            .ok()
            .filter(|s| !s.is_empty())?;
        let refresh_secs: u64 = std::env::var("JWT_JWKS_REFRESH_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let cache = JwksCache {
            inner: Arc::new(RwLock::new(HashMap::new())),
        };
        let _ = GLOBAL.set(cache.clone());

        let bg = cache.clone();
        tokio::spawn(async move {
            // First fetch at boot — log but don't panic on failure.
            if let Err(e) = bg.refresh(&url).await {
                tracing::warn!(error = %e, %url, "initial JWKS fetch failed; will retry");
            }
            let mut ticker = tokio::time::interval(Duration::from_secs(refresh_secs));
            ticker.tick().await; // skip immediate
            loop {
                ticker.tick().await;
                if let Err(e) = bg.refresh(&url).await {
                    tracing::warn!(error = %e, %url, "JWKS refresh failed; keeping cached keys");
                }
            }
        });

        Some(cache)
    }

    pub fn global() -> Option<&'static JwksCache> {
        GLOBAL.get()
    }

    async fn refresh(&self, url: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let doc: JwksDoc = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut next: HashMap<String, DecodingKey> = HashMap::new();
        for k in doc.keys {
            // Only RS256 keys for now; HS256 doesn't ship public material
            // in JWKS for obvious reasons.
            if k.alg.as_deref().unwrap_or("RS256") != "RS256" {
                continue;
            }
            let (Some(n), Some(e)) = (k.n.as_ref(), k.e.as_ref()) else {
                continue;
            };
            if let Ok(key) = DecodingKey::from_rsa_components(n, e) {
                next.insert(k.kid, key);
            }
        }
        let count = next.len();
        let mut guard = self.inner.write().await;
        *guard = next;
        tracing::info!(keys = count, %url, "JWKS refreshed");
        Ok(())
    }

    /// Look up a `DecodingKey` by JWT `kid` header. Returns `None` if the
    /// kid is unknown — caller should then fall back to `JWT_PUBLIC_KEY_PEM`.
    pub async fn key_for_kid(&self, kid: &str) -> Option<DecodingKey> {
        self.inner.read().await.get(kid).cloned()
    }
}

/// Algorithm hint for the JWKS path; we hard-code RS256 here because all
/// keys are filtered to that during refresh.
#[allow(dead_code)] // referenced by future direct-verify paths and external consumers
pub const JWKS_ALG: Algorithm = Algorithm::RS256;
