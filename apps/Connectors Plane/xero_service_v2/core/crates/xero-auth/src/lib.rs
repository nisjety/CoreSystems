//! `xero-auth` — Xero authentication flows.
//!
//! Two flows are supported:
//! * [`custom_connection`] — the `client_credentials` grant used by the
//!   stateless raw→GCS uploader (machine-to-machine, no user, no refresh token).
//! * [`authorization_code`] — the user-facing Authorization Code + PKCE flow
//!   used by the onboarding BFF (a person authorizes org(s); we capture a
//!   refresh token + the connected tenants).

pub mod authorization_code;
pub mod custom_connection;
pub mod identity;
pub mod pkce_provider;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use authorization_code::{
    authorize_url, authorize_url_with_nonce, exchange_code, fetch_connections, random_state,
    refresh, PkceChallenge, TokenResponse, XeroConnection,
};
pub use custom_connection::{CustomConnectionClient, MultiTenantCustomConnectionClient};
pub use pkce_provider::PkceTokenProvider;

// ── Token data ────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
    pub tenant_id: String,
}

// Manual `Debug` that redacts the bearer/refresh tokens so a stray `{:?}`
// (ours or a future caller's) can never leak credentials into logs.
impl std::fmt::Debug for TokenData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenData")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("tenant_id", &self.tenant_id)
            .finish()
    }
}

impl TokenData {
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Return true when expiry is within `buffer_secs` seconds.
    pub fn needs_refresh(&self, buffer_secs: i64) -> bool {
        let refresh_at = self.expires_at - chrono::Duration::seconds(buffer_secs);
        Utc::now() >= refresh_at
    }
}

// ── Xero OAuth endpoints ───────────────────────────────────────────────────────

/// Xero token endpoint — used by every grant (`client_credentials`,
/// `authorization_code`, `refresh_token`).
pub const TOKEN_URL: &str = "https://identity.xero.com/connect/token";

/// Xero authorize endpoint — user-facing OAuth2 + PKCE redirect target.
pub const AUTHORIZE_URL: &str = "https://login.xero.com/identity/connect/authorize";

/// Xero connections endpoint — lists the tenants an access token can act on.
pub const CONNECTIONS_URL: &str = "https://api.xero.com/connections";

// ── TokenProvider trait ─────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait TokenProvider: Send + Sync {
    async fn get_valid_token(&self, tenant_id: &str) -> xero_common::Result<crate::TokenData>;
}

#[async_trait::async_trait]
impl TokenProvider for custom_connection::CustomConnectionClient {
    async fn get_valid_token(&self, tenant_id: &str) -> xero_common::Result<crate::TokenData> {
        if tenant_id != self.tenant_id() {
            return Err(xero_common::Error::Auth(
                "custom connection tenant mismatch".to_owned(),
            ));
        }

        self.fetch_token().await
    }
}

#[async_trait::async_trait]
impl TokenProvider for custom_connection::MultiTenantCustomConnectionClient {
    async fn get_valid_token(&self, tenant_id: &str) -> xero_common::Result<crate::TokenData> {
        self.fetch_token_for_tenant(tenant_id).await
    }
}
