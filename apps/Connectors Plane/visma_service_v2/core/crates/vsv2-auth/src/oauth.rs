use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::token_cache::CachedToken;
use crate::Scope;
use vsv2_common::{Error, Result, TenantId};

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Visma Connect OAuth client.
///
/// Implements the `client_credentials` grant with Visma's per-tenant
/// `tenant_id` form parameter (mirrors v1 `auth/visma_oauth_client.py`).
/// Tokens are cached per `(tenant_id, scope)` in-memory; Redis-backed
/// sharing is a Phase 1b extension.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    /// Logical → wire scope strings.
    pub scope_map: std::collections::HashMap<Scope, String>,
    http: reqwest::Client,
    cache: Arc<Mutex<std::collections::HashMap<CacheKey, CachedToken>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    tenant: String,
    scope: Scope,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    #[allow(dead_code)]
    token_type: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

impl OAuthClient {
    pub fn new(
        token_url: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build()
            .expect("reqwest client builds");
        Self {
            token_url: token_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            scope_map: Default::default(),
            http,
            cache: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub fn with_scope(mut self, scope: Scope, wire: impl Into<String>) -> Self {
        self.scope_map.insert(scope, wire.into());
        self
    }

    /// Drop the cached token for `tenant` and `scope`, so the next `token_for` asks for a new one.
    ///
    /// For a token the server rejected although the cache still counted it fresh: it was revoked,
    /// or the process was paused for longer than the token lives (a sleeping laptop, a suspended
    /// container) and the request went out on a token that expired while it was in flight.
    pub async fn forget_token(&self, tenant: &TenantId, scope: Scope) {
        let key = CacheKey {
            tenant: tenant.as_str().to_string(),
            scope,
        };
        self.cache.lock().await.remove(&key);
    }

    /// Fetch a Bearer token for the given tenant + logical scope.
    /// Cache hits return the in-memory token until 60s before expiry.
    pub async fn token_for(&self, tenant: &TenantId, scope: Scope) -> Result<String> {
        let key = CacheKey {
            tenant: tenant.as_str().to_string(),
            scope,
        };

        {
            let cache = self.cache.lock().await;
            if let Some(tok) = cache.get(&key) {
                if tok.is_fresh(60) {
                    debug!(tenant = %tenant, scope = ?scope, "oauth: cache hit");
                    return Ok(tok.access_token.clone());
                }
            }
        }

        let wire_scope = self
            .scope_map
            .get(&scope)
            .ok_or_else(|| Error::Auth(format!("no wire scope configured for {scope:?}")))?;

        info!(tenant = %tenant, scope = ?scope, "oauth: fetching new token");

        let mut form: Vec<(&str, String)> = vec![
            ("grant_type", "client_credentials".into()),
            ("scope", wire_scope.clone()),
            ("tenant_id", tenant.as_str().to_string()),
        ];
        // `client_secret_post` style
        form.push(("client_id", self.client_id.clone()));
        form.push(("client_secret", self.client_secret.clone()));

        let resp = self
            .http
            .post(&self.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::Http(format!("oauth token request: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Error::Http(format!("read oauth body: {e}")))?;

        if !status.is_success() {
            // Error bodies are short and informative (`invalid_scope`, ...), but bound them and strip
            // control characters: this text reaches logs and, via the MCP server, an LLM.
            let shown = vsv2_common::text::printable(&body, 300);
            warn!(status = %status, body = %shown, "oauth: non-success");
            return Err(Error::Auth(format!(
                "oauth token endpoint returned {status}: {shown}"
            )));
        }

        let parsed: TokenResponse = serde_json::from_str(&body)
            // A 200 body is where an access_token lives: never echo it.
            .map_err(|e| Error::Auth(format!("parse oauth response: {e} (body withheld)")))?;

        let cached = CachedToken {
            access_token: parsed.access_token.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(parsed.expires_in),
        };

        {
            let mut cache = self.cache.lock().await;
            cache.insert(key, cached);
        }
        Ok(parsed.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(url: &str) -> OAuthClient {
        OAuthClient::new(format!("{url}/token"), "cid", "csecret")
            .with_scope(Scope::Erp, "erp:read")
    }

    #[tokio::test]
    async fn a_malformed_success_body_is_never_echoed_into_the_error() {
        let mut s = mockito::Server::new_async().await;
        let _m = s
            .mock("POST", "/token")
            .with_body(r#"{"access_token":"SUPER-SECRET-TOKEN","expires_in":"soon"}"#)
            .create_async()
            .await;
        let err = client(&s.url())
            .token_for(&TenantId::new("t"), Scope::Erp)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            !err.contains("SUPER-SECRET-TOKEN"),
            "token leaked into error: {err}"
        );
        assert!(err.contains("parse oauth response"), "{err}");
    }

    #[tokio::test]
    async fn error_bodies_are_bounded_and_printable() {
        let mut s = mockito::Server::new_async().await;
        let body = format!("\u{0}\u{1b}[31m{}", "x".repeat(5_000));
        let _m = s
            .mock("POST", "/token")
            .with_status(400)
            .with_body(body)
            .create_async()
            .await;
        let err = client(&s.url())
            .token_for(&TenantId::new("t"), Scope::Erp)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("400"), "{err}");
        assert!(
            err.chars().count() < 600,
            "bounded, got {}",
            err.chars().count()
        );
        assert!(
            !err.contains('\u{0}') && !err.contains('\u{1b}'),
            "control chars stripped"
        );
    }
}
