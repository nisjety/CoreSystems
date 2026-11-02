//! BFF runtime configuration (PKCE onboarding). Loaded from env after dotenv.

use std::env;

use anyhow::{anyhow, Context};
use xero_common::{load_dotenv, load_dotenv_file};

/// Default read-only scope set for onboarding.
///
/// `offline_access` is REQUIRED to receive a refresh token; `openid profile
/// email` identify the signing-in user. The `accounting.*.read` scopes match
/// what the downstream sync reads.
const DEFAULT_SCOPES: &str = "openid profile email offline_access \
accounting.transactions.read accounting.contacts.read accounting.settings.read \
accounting.reports.read accounting.journals.read accounting.attachments.read \
accounting.budgets.read";

#[derive(Debug, Clone)]
pub struct BffConfig {
    /// host:port to bind (default 127.0.0.1:8080).
    pub bind_addr: String,
    /// Xero OAuth2 (PKCE) client id.
    pub client_id: String,
    /// Client secret for a confidential Xero "Web app" (PKCE + secret).
    /// `None` for a pure public client.
    pub client_secret: Option<String>,
    /// Redirect URI registered in the Xero app — must exactly match.
    pub redirect_uri: String,
    /// Space-separated OAuth scopes to request.
    pub scopes: String,
    /// base64 of >= 64 random bytes — encrypts session/PKCE cookies.
    pub cookie_key_b64: String,
    /// SPA path to land on after a successful connect.
    pub spa_connected_path: String,
    /// SPA path to land on after a failed/denied connect.
    pub spa_error_path: String,
    /// Set the `Secure` flag on cookies (enable behind HTTPS in prod).
    pub secure_cookies: bool,
    /// Optional directory of built SPA assets to serve (single-origin deploy).
    /// When set, non-API paths fall back to it (index.html for SPA routing).
    pub static_dir: Option<String>,
}

impl BffConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        // Dotenv is first-wins: process env > .env.bff > .env.
        load_dotenv_file(".env.bff").map_err(|error| anyhow!(error.to_string()))?;
        load_dotenv().map_err(|error| anyhow!(error.to_string()))?;

        let client_id = required("XERO_PKCE_CLIENT_ID")?;
        let redirect_uri = required("XERO_REDIRECT_URI")?;
        let cookie_key_b64 =
            required("XERO_BFF_COOKIE_KEY").context("generate with: openssl rand -base64 64")?;

        Ok(Self {
            bind_addr: optional("XERO_BFF_BIND").unwrap_or_else(|| "127.0.0.1:8080".into()),
            client_id,
            client_secret: optional("XERO_PKCE_CLIENT_SECRET"),
            redirect_uri,
            scopes: optional("XERO_PKCE_SCOPES").unwrap_or_else(|| DEFAULT_SCOPES.into()),
            cookie_key_b64,
            spa_connected_path: optional("XERO_SPA_CONNECTED_PATH")
                .unwrap_or_else(|| "/connected".into()),
            spa_error_path: optional("XERO_SPA_ERROR_PATH").unwrap_or_else(|| "/auth/error".into()),
            secure_cookies: optional("XERO_BFF_SECURE_COOKIES")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            static_dir: optional("XERO_BFF_STATIC_DIR"),
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    optional(key).ok_or_else(|| anyhow!("missing required env var: {key}"))
}

fn optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn default_scopes_cover_budget_details() {
        assert!(DEFAULT_SCOPES
            .split_whitespace()
            .any(|scope| scope == "accounting.budgets.read"));
        assert!(!DEFAULT_SCOPES
            .split_whitespace()
            .any(|scope| scope == "paymentservices"));
    }

    #[test]
    fn malformed_optional_env_file_is_not_ignored() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("xero-bff-env-{}-{nonce}", std::process::id()));
        std::fs::write(&path, "XERO_PKCE_CLIENT_SECRET=private value\n").unwrap();
        let result = xero_common::load_dotenv_path(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(result.is_err());
        assert!(!format!("{:?}", result.unwrap_err()).contains("private value"));
    }
}
