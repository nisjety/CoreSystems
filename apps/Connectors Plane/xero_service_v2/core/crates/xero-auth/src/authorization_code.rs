//! Authorization Code + PKCE flow for Xero (user-facing onboarding).
//!
//! Used by the onboarding BFF: a user authorizes one or more Xero
//! organisations, and we capture a refresh token plus the connected tenants so
//! the backend can keep them synced. This is a PKCE **public client** — no
//! client secret is ever sent.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{AUTHORIZE_URL, CONNECTIONS_URL, TOKEN_URL};

/// A PKCE verifier/challenge pair (S256 method).
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The secret verifier — kept server-side, sent only at token exchange.
    pub verifier: String,
    /// The public challenge — `base64url(sha256(verifier))`, sent at authorize.
    pub challenge: String,
}

impl PkceChallenge {
    /// Generate a fresh pair from 32 bytes of randomness (43-char verifier).
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = Self::challenge_for(&verifier);
        Self {
            verifier,
            challenge,
        }
    }

    /// S256 challenge = `base64url(sha256(verifier))`.
    pub fn challenge_for(verifier: &str) -> String {
        let digest = Sha256::digest(verifier.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

/// Random URL-safe `state` for CSRF protection on the authorize round-trip.
pub fn random_state() -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Build the Xero authorize URL to redirect the user's browser to.
pub fn authorize_url(
    client_id: &str,
    redirect_uri: &str,
    scopes: &str,
    state: &str,
    challenge: &str,
) -> String {
    let mut url = Url::parse(AUTHORIZE_URL).expect("AUTHORIZE_URL is a valid const URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scopes)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    url.to_string()
}

pub fn authorize_url_with_nonce(
    client_id: &str,
    redirect_uri: &str,
    scopes: &str,
    state: &str,
    challenge: &str,
    nonce: &str,
) -> String {
    let mut url = Url::parse(&authorize_url(
        client_id,
        redirect_uri,
        scopes,
        state,
        challenge,
    ))
    .expect("Xero authorize URL is valid");
    url.query_pairs_mut().append_pair("nonce", nonce);
    url.into()
}

/// Token endpoint response (`authorization_code` and `refresh_token` grants).
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Present when `offline_access` was granted; empty otherwise.
    #[serde(default)]
    pub refresh_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

impl TokenResponse {
    /// Absolute expiry, computed from `expires_in` at parse time.
    pub fn expires_at(&self) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(self.expires_in)
    }

    /// Granted scopes, split from the space-delimited `scope` string.
    pub fn scopes(&self) -> Vec<String> {
        self.scope
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect()
    }
}

/// One authorized Xero tenant, from `GET /connections`.
#[derive(Debug, Clone, Deserialize)]
pub struct XeroConnection {
    /// The connection id (distinct from the tenant id).
    pub id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    #[serde(rename = "tenantType")]
    pub tenant_type: String,
    #[serde(rename = "tenantName", default)]
    pub tenant_name: Option<String>,
}

/// Exchange an authorization `code` (+ PKCE `verifier`) for tokens.
///
/// `client_secret` is `None` for a public client and `Some(..)` for a
/// confidential client (Xero "Web app"); when set it is sent in the form body,
/// as Xero's PKCE flow expects.
pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code: &str,
    verifier: &str,
) -> xero_common::Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    post_token(http, &form).await
}

/// Exchange a refresh token for a fresh access/refresh token pair.
///
/// Pass `client_secret` for a confidential client (see [`exchange_code`]).
pub async fn refresh(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> xero_common::Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    post_token(http, &form).await
}

async fn post_token(
    http: &reqwest::Client,
    form: &[(&str, &str)],
) -> xero_common::Result<TokenResponse> {
    let resp = http
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| xero_common::Error::Auth(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| xero_common::Error::Auth(format!("token read error: {e}")))?;

    if !status.is_success() {
        return Err(xero_common::Error::Auth(format!(
            "token endpoint failed (HTTP {status}): {body}"
        )));
    }

    serde_json::from_str(&body)
        .map_err(|e| xero_common::Error::Auth(format!("token parse error: {e}")))
}

/// Fetch the tenants authorized for an access token.
pub async fn fetch_connections(
    http: &reqwest::Client,
    access_token: &str,
) -> xero_common::Result<Vec<XeroConnection>> {
    let resp = http
        .get(CONNECTIONS_URL)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| xero_common::Error::XeroApi(e.to_string()))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| xero_common::Error::XeroApi(format!("connections read error: {e}")))?;

    if !status.is_success() {
        return Err(xero_common::Error::XeroApi(format!(
            "connections fetch failed (HTTP {status}): {body}"
        )));
    }

    serde_json::from_str(&body)
        .map_err(|e| xero_common::Error::XeroApi(format!("connections parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        // RFC 7636 Appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = PkceChallenge::challenge_for(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_pair_is_self_consistent() {
        let pkce = PkceChallenge::generate();
        assert_eq!(pkce.challenge, PkceChallenge::challenge_for(&pkce.verifier));
        assert!(pkce.verifier.len() >= 43, "verifier too short for PKCE");
    }

    #[test]
    fn authorize_url_has_required_params() {
        let u = authorize_url(
            "cid",
            "https://app.example/api/auth/callback",
            "openid offline_access",
            "state123",
            "chal123",
        );
        assert!(u.starts_with(AUTHORIZE_URL));
        assert!(u.contains("response_type=code"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("client_id=cid"));
        assert!(u.contains("state=state123"));
        assert!(u.contains("code_challenge=chal123"));
    }

    #[test]
    fn nonce_aware_authorize_url_binds_the_identity_response() {
        let url = authorize_url_with_nonce(
            "cid",
            "https://app.example/api/auth/callback",
            "openid profile email",
            "state123",
            "chal123",
            "nonce123",
        );
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(key, _)| key == "nonce")
                .map(|(_, value)| value.into_owned()),
            Some("nonce123".into())
        );
    }

    #[test]
    fn states_are_unique() {
        assert_ne!(random_state(), random_state());
    }
}
