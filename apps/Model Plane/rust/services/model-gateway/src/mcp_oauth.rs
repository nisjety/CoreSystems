//! OAuth 2.1 + Dynamic Client Registration (RFC 7591) client for connecting
//! authenticated MCP servers (e.g. Visma Net) with nothing but their URL —
//! no pre-registered app, no client secret handed to us by the user.
//!
//! Discovery chain per the MCP Authorization spec: a bare `initialize` call
//! against the server 401s with `WWW-Authenticate: Bearer
//! resource_metadata="..."`; that document (RFC 9728, Protected Resource
//! Metadata) names the authorization server(s); the authorization server's
//! own metadata document (RFC 8414) names the `registration_endpoint`,
//! `authorization_endpoint`, and `token_endpoint`. DCR mints a fresh
//! `client_id` per server the first time anyone in an org connects it.
//!
//! This module is pure protocol logic (HTTP + PKCE/state generation) with no
//! storage or HTTP-route concerns — see `http_routes.rs` for the
//! `/v1/mcp/servers/oauth/*` endpoints that drive this, and capability-core
//! for where the resulting tokens are durably encrypted and stored.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum McpOAuthError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("discovery failed: {0}")]
    Discovery(String),
    #[error("dynamic client registration was rejected: {0}")]
    RegistrationRejected(String),
    #[error("token exchange was rejected: {0}")]
    TokenExchangeRejected(String),
}

/// RFC 9728 OAuth 2.0 Protected Resource Metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct ProtectedResourceMetadata {
    #[serde(default)]
    pub resource: String,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// RFC 8414 OAuth 2.0 Authorization Server Metadata (only the fields this
/// client needs).
#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizationServerMetadata {
    #[serde(default)]
    pub authorization_endpoint: String,
    #[serde(default)]
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
}

/// RFC 7591 Dynamic Client Registration response (only the fields used).
#[derive(Debug, Clone, Deserialize)]
pub struct ClientRegistration {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RegistrationRequest<'a> {
    client_name: &'a str,
    redirect_uris: Vec<&'a str>,
    grant_types: Vec<&'a str>,
    response_types: Vec<&'a str>,
    token_endpoint_auth_method: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds until expiry, per RFC 6749 §5.1.
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Fetch `{origin}/.well-known/oauth-protected-resource{path}` for the given
/// MCP server URL, falling back to the bare-origin form — some servers only
/// publish the document at the root, others (like Visma) scope it to the
/// resource path.
pub async fn discover_protected_resource(
    client: &reqwest::Client,
    server_url: &str,
) -> Result<ProtectedResourceMetadata, McpOAuthError> {
    let parsed = reqwest::Url::parse(server_url)
        .map_err(|e| McpOAuthError::Discovery(format!("invalid server_url: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(McpOAuthError::Discovery(
            "server_url must be https".to_owned(),
        ));
    }
    let origin = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or_default()
    );
    let path = parsed.path().trim_end_matches('/');
    let mut candidates = Vec::new();
    if !path.is_empty() {
        candidates.push(format!(
            "{origin}/.well-known/oauth-protected-resource{path}"
        ));
    }
    candidates.push(format!("{origin}/.well-known/oauth-protected-resource"));

    let mut last_error = String::new();
    for url in candidates {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .json::<ProtectedResourceMetadata>()
                    .await
                    .map_err(|e| McpOAuthError::Discovery(format!("{url}: {e}")));
            }
            Ok(response) => last_error = format!("{url}: HTTP {}", response.status()),
            Err(error) => last_error = format!("{url}: {error}"),
        }
    }
    Err(McpOAuthError::Discovery(format!(
        "no oauth-protected-resource metadata found ({last_error})"
    )))
}

/// Fetch `{authorization_server}/.well-known/oauth-authorization-server`,
/// falling back to the OIDC discovery path for servers that only publish
/// that document.
pub async fn discover_authorization_server(
    client: &reqwest::Client,
    authorization_server: &str,
) -> Result<AuthorizationServerMetadata, McpOAuthError> {
    if !authorization_server.starts_with("https://") {
        return Err(McpOAuthError::Discovery(
            "authorization_server must be https".to_owned(),
        ));
    }
    let base = authorization_server.trim_end_matches('/');
    let mut last_error = String::new();
    for suffix in [
        "/.well-known/oauth-authorization-server",
        "/.well-known/openid-configuration",
    ] {
        let url = format!("{base}{suffix}");
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .json::<AuthorizationServerMetadata>()
                    .await
                    .map_err(|e| McpOAuthError::Discovery(format!("{url}: {e}")));
            }
            Ok(response) => last_error = format!("{url}: HTTP {}", response.status()),
            Err(error) => last_error = format!("{url}: {error}"),
        }
    }
    Err(McpOAuthError::Discovery(format!(
        "no authorization-server metadata found ({last_error})"
    )))
}

/// RFC 7591 Dynamic Client Registration — mints a fresh `client_id` (and
/// possibly `client_secret`) scoped to `redirect_uri`. No pre-existing app
/// registration is required; this is what lets a server be connected with
/// nothing but its URL.
pub async fn register_client(
    client: &reqwest::Client,
    registration_endpoint: &str,
    redirect_uri: &str,
) -> Result<ClientRegistration, McpOAuthError> {
    if !registration_endpoint.starts_with("https://") {
        return Err(McpOAuthError::RegistrationRejected(
            "registration_endpoint must be https".to_owned(),
        ));
    }
    let body = RegistrationRequest {
        client_name: "Velion",
        redirect_uris: vec![redirect_uri],
        grant_types: vec!["authorization_code", "refresh_token"],
        response_types: vec!["code"],
        // Public client (PKCE-secured, no client_secret to protect) unless
        // the server insists otherwise — it may still issue one; we keep it
        // if present but never require it.
        token_endpoint_auth_method: "none",
    };
    let response = client
        .post(registration_endpoint)
        .json(&body)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(McpOAuthError::RegistrationRejected(format!(
            "{status}: {text}"
        )));
    }
    response
        .json::<ClientRegistration>()
        .await
        .map_err(|e| McpOAuthError::RegistrationRejected(e.to_string()))
}

/// A PKCE (RFC 7636) verifier/challenge pair, `S256` method.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// Generate a cryptographically random PKCE verifier (64 raw bytes
/// base64url-encodes to 86 chars, within the spec's 43-128 bound) and its
/// `S256` challenge.
#[must_use]
pub fn generate_pkce() -> Pkce {
    let mut raw = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut raw);
    let verifier = URL_SAFE_NO_PAD.encode(raw);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    Pkce { verifier, challenge }
}

/// Generate a cryptographically random CSRF `state` value.
#[must_use]
pub fn generate_state() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    URL_SAFE_NO_PAD.encode(raw)
}

/// Build the authorization redirect URL the browser is sent to.
#[must_use]
pub fn build_authorization_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
    scopes: &[String],
) -> String {
    let Ok(mut url) = reqwest::Url::parse(authorization_endpoint) else {
        return String::new();
    };
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", state);
        query.append_pair("code_challenge", code_challenge);
        query.append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            query.append_pair("scope", &scopes.join(" "));
        }
    }
    url.to_string()
}

/// Exchange an authorization `code` for tokens (RFC 6749 §4.1.3 + PKCE).
pub async fn exchange_code(
    client: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, McpOAuthError> {
    if !token_endpoint.starts_with("https://") {
        return Err(McpOAuthError::TokenExchangeRejected(
            "token_endpoint must be https".to_owned(),
        ));
    }
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ];
    let response = client.post(token_endpoint).form(&params).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(McpOAuthError::TokenExchangeRejected(format!(
            "{status}: {text}"
        )));
    }
    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| McpOAuthError::TokenExchangeRejected(e.to_string()))
}

/// Refresh an access token using a stored `refresh_token` (RFC 6749 §6).
pub async fn refresh_access_token(
    client: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, McpOAuthError> {
    if !token_endpoint.starts_with("https://") {
        return Err(McpOAuthError::TokenExchangeRejected(
            "token_endpoint must be https".to_owned(),
        ));
    }
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];
    let response = client.post(token_endpoint).form(&params).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(McpOAuthError::TokenExchangeRejected(format!(
            "{status}: {text}"
        )));
    }
    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| McpOAuthError::TokenExchangeRejected(e.to_string()))
}

/// Resolve a stored OAuth access token for (`org_id`, `server_id`) from
/// capability-core's system-of-record, authenticating with a shared
/// Model-Plane-local service secret rather than a per-user bearer.
///
/// Deliberately soft-fail (`None`, never an error): this is on the
/// `tools/call` dispatch path for every MCP server, including ones that
/// were never OAuth-connected at all. An unconfigured secret, a network
/// hiccup, or "no tokens stored for this server" must all fall back to
/// `server.token`-based auth exactly as before, never break the call.
///
/// Deliberately narrow-scoped: `service_token` proves only "this is a
/// trusted internal caller," not a specific org — unlike the minted,
/// per-user JWTs every other gateway->capability-core call uses. The
/// (`server_id`, `org_id`) pair still has to match a real stored row (and
/// its AAD-bound ciphertext, see capability-core's vault) for anything to
/// come back, so a caller can only ever resolve tokens for servers that
/// genuinely belong to the org it asserts.
pub async fn resolve_stored_oauth_token(
    client: &reqwest::Client,
    capability_core_base_url: &str,
    service_token: &str,
    org_id: &str,
    server_id: &str,
) -> Option<String> {
    if capability_core_base_url.is_empty() || service_token.is_empty() {
        return None;
    }
    let mut url = reqwest::Url::parse(&format!(
        "{capability_core_base_url}/api/v1/mcp/{server_id}/oauth-token"
    ))
    .ok()?;
    url.query_pairs_mut().append_pair("org_id", org_id);
    let response = client
        .get(url)
        .header("X-Mcp-Service-Token", service_token)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    let token = body.get("access_token")?.as_str()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_deterministic_sha256_of_verifier() {
        let pkce = generate_pkce();
        let mut hasher = Sha256::new();
        hasher.update(pkce.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn pkce_verifier_meets_spec_length_bounds() {
        let pkce = generate_pkce();
        assert!(pkce.verifier.len() >= 43 && pkce.verifier.len() <= 128);
    }

    #[test]
    fn pkce_verifier_and_challenge_are_url_safe() {
        let pkce = generate_pkce();
        let is_url_safe = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        assert!(is_url_safe(&pkce.verifier));
        assert!(is_url_safe(&pkce.challenge));
    }

    #[test]
    fn state_is_url_safe_and_nonempty() {
        let state = generate_state();
        assert!(!state.is_empty());
        assert!(state
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn two_generated_states_are_not_equal() {
        // Not a proof of randomness, but catches a broken RNG/constant bug.
        assert_ne!(generate_state(), generate_state());
    }

    #[test]
    fn authorization_url_includes_pkce_and_state() {
        let url = build_authorization_url(
            "https://auth.example.test/authorize",
            "client-123",
            "https://velion.example.test/callback",
            "state-abc",
            "challenge-xyz",
            &["read".to_owned(), "write".to_owned()],
        );
        assert!(url.contains("client_id=client-123"));
        assert!(url.contains("state=state-abc"));
        assert!(url.contains("code_challenge=challenge-xyz"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("response_type=code"));
    }

    #[test]
    fn authorization_url_omits_scope_when_empty() {
        let url = build_authorization_url(
            "https://auth.example.test/authorize",
            "client-123",
            "https://velion.example.test/callback",
            "state-abc",
            "challenge-xyz",
            &[],
        );
        assert!(!url.contains("scope="));
    }

    // resolve_stored_oauth_token must never attempt a network call when
    // unconfigured — these hit the early-return before any `.send()`, so
    // they stay fast/deterministic without a mock server.
    #[tokio::test]
    async fn resolve_stored_oauth_token_short_circuits_without_base_url() {
        let client = reqwest::Client::new();
        let token =
            resolve_stored_oauth_token(&client, "", "shared-secret", "org-1", "srv-1").await;
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn resolve_stored_oauth_token_short_circuits_without_service_token() {
        let client = reqwest::Client::new();
        let token = resolve_stored_oauth_token(
            &client,
            "http://capability-core:8085",
            "",
            "org-1",
            "srv-1",
        )
        .await;
        assert!(token.is_none());
    }
}
