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
//! This module is protocol logic (HTTP + PKCE/state generation) with no
//! HTTP-route concerns — see `http_routes.rs` for the
//! `/v1/mcp/servers/oauth/*` endpoints that drive this, and capability-core
//! for where the resulting tokens are durably encrypted and stored. The one
//! exception is `resolve_stored_oauth_token` at the bottom, which reads that
//! store (and writes back a refreshed token) because refreshing is a protocol
//! operation capability-core deliberately does not perform itself.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::warn;

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
        client_name: "Verevon",
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

/// Seconds of headroom demanded of a stored token's `expires_at` before it is
/// handed to a caller. The token is not used at this instant: the caller still
/// has to build the request, complete the MCP `initialize` handshake and issue
/// the actual `tools/list`/`tools/call`, any of which would 401 if the token
/// died in between. A minute comfortably covers that round trip while staying
/// far below a typical hour-long token lifetime, so it never forces a refresh
/// that was not already almost due.
const OAUTH_EXPIRY_SKEW_SECS: i64 = 60;

/// Decrypted OAuth material as capability-core's internal resolve returns it.
/// Everything except `expires_at` (a nullable column, omitted when unset) is
/// always present, empty-string when the authorization server never gave it.
#[derive(Debug, Clone, Deserialize)]
struct StoredOAuthToken {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    scope: String,
    /// RFC 3339.
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    token_endpoint: String,
    #[serde(default)]
    client_id: String,
}

/// Header carrying the per-org derived internal service token on both
/// `/api/v1/internal/mcp/oauth-token(s)` calls.
const MCP_SERVICE_TOKEN_HEADER: &str = "X-Mcp-Service-Token";

/// Derive the per-org internal service token presented to capability-core's
/// unauthenticated `/api/v1/internal/mcp/oauth-token(s)` routes:
///
/// ```text
/// presented = lowercase_hex( HMAC-SHA256( key = MCP_OAUTH_SERVICE_TOKEN,
///                                        message = org_id ) )
/// ```
///
/// Honest security property: this bounds the blast radius of an *intercepted*
/// credential. A header captured off one request — a proxy access log, a
/// tcpdump on the Model-Plane network, a leaked trace — is replayable only
/// against the single org it was minted for, and is worthless for every other
/// tenant. It does NOT mitigate compromise of the root
/// `MCP_OAUTH_SERVICE_TOKEN` itself: whoever holds that value can derive the
/// token for any org at will, exactly as before. The root secret must still be
/// treated as a tenant-wide credential.
///
/// `org_id` is trimmed before hashing because capability-core authorizes
/// against the `TrimSpace`d query parameter and derives its expectation from
/// that same trimmed value — the two sides must hash byte-identical input or
/// every call 403s. capability-core's twin is `expectedInternalServiceToken`
/// in `services/capability-core/internal/api/registry_apis.go`; the
/// known-answer vector in this module's tests is duplicated in that package's
/// tests so any drift between the two implementations fails a test instead of
/// breaking auth in production.
fn derive_org_scoped_service_token(root_secret: &str, org_id: &str) -> String {
    // `new_from_slice` only errors for key sizes a given MAC rejects; HMAC
    // accepts any length, so this cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(root_secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(org_id.trim().as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Lowercase, no separators — the wire format both sides agree on.
        hex.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble is < 16"));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble is < 16"));
    }
    hex
}

/// Resolve a usable OAuth access token for (`org_id`, `server_id`) from
/// capability-core's system-of-record, authenticating with a shared
/// Model-Plane-local service secret rather than a per-user bearer. Refreshes
/// the stored token first when it has expired (or is about to), persisting the
/// result back so the next caller does not have to.
///
/// Deliberately soft-fail (`None`, never an error): this is on the
/// `tools/call` dispatch path for every MCP server, including ones that
/// were never OAuth-connected at all. An unconfigured secret, a network
/// hiccup, or "no tokens stored for this server" must all fall back to
/// `server.token`-based auth exactly as before, never break the call.
///
/// Also `None` — rather than the stored value — when the token is provably
/// expired and cannot be refreshed. Returning a dead token guarantees a
/// downstream 401 that reaches the user as "I have no access to this system,"
/// which is indistinguishable from never having connected it; `None` at least
/// leaves the honest "not connected" path intact.
///
/// Deliberately narrow-scoped: `service_token` is the ROOT secret, never sent
/// on the wire. What is presented is
/// `derive_org_scoped_service_token(service_token, org_id)`, so the credential
/// crossing the network proves "trusted internal caller, for THIS org" rather
/// than the unbounded "trusted internal caller" a raw shared secret proved —
/// see that function for the exact property this buys and what it does not.
/// The (`server_id`, `org_id`) pair still has to match a real stored row (and
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
    // /api/v1/internal/... (not /api/v1/mcp/{id}/oauth-token) — capability-core
    // wraps its normal MCP routes in a blanket per-user JWT middleware this
    // call has no bearer to satisfy, so this endpoint is deliberately mounted
    // outside that wrapper, gated only by the service token below.
    let mut url = reqwest::Url::parse(&format!(
        "{capability_core_base_url}/api/v1/internal/mcp/oauth-token"
    ))
    .ok()?;
    url.query_pairs_mut()
        .append_pair("server_id", server_id)
        .append_pair("org_id", org_id);
    let response = client
        .get(url)
        .header(
            MCP_SERVICE_TOKEN_HEADER,
            derive_org_scoped_service_token(service_token, org_id),
        )
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let stored: StoredOAuthToken = response.json().await.ok()?;
    let access_token = stored.access_token.trim();
    if access_token.is_empty() {
        return None;
    }
    if stored_access_token_is_live(stored.expires_at.as_deref(), Utc::now()) {
        return Some(access_token.to_owned());
    }
    refresh_stored_oauth_token(
        client,
        capability_core_base_url,
        service_token,
        org_id,
        server_id,
        &stored,
    )
    .await
}

/// Whether a stored access token can be handed out as-is.
///
/// A missing or unparseable `expires_at` means we have no proof the token is
/// dead, so it passes through unchanged: an authorization server is free to
/// omit `expires_in` (leaving the column NULL), and withholding a token that
/// may well be valid would break those servers outright. Only a timestamp we
/// can read AND that has already passed — or falls inside
/// `OAUTH_EXPIRY_SKEW_SECS` — forces a refresh.
fn stored_access_token_is_live(expires_at: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(raw) = expires_at.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let Ok(expiry) = DateTime::parse_from_rfc3339(raw) else {
        warn!(
            expires_at = raw,
            "mcp oauth: unparseable stored token expiry"
        );
        return true;
    };
    expiry.with_timezone(&Utc) > now + chrono::Duration::seconds(OAUTH_EXPIRY_SKEW_SECS)
}

/// Trade the stored `refresh_token` for a fresh access token and write the
/// result back to capability-core. `None` on any failure — see
/// `resolve_stored_oauth_token`'s contract for why a known-dead token is never
/// returned as a consolation.
async fn refresh_stored_oauth_token(
    client: &reqwest::Client,
    capability_core_base_url: &str,
    service_token: &str,
    org_id: &str,
    server_id: &str,
    stored: &StoredOAuthToken,
) -> Option<String> {
    let refresh_token = stored.refresh_token.trim();
    let token_endpoint = stored.token_endpoint.trim();
    if refresh_token.is_empty() || token_endpoint.is_empty() {
        warn!(
            server_id,
            has_refresh_token = !refresh_token.is_empty(),
            has_token_endpoint = !token_endpoint.is_empty(),
            "mcp oauth: stored token expired and cannot be refreshed; the server must be reconnected"
        );
        return None;
    }
    let refreshed = match refresh_access_token(
        client,
        token_endpoint,
        stored.client_id.trim(),
        refresh_token,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            warn!(
                server_id,
                error = %error,
                "mcp oauth: refresh rejected; the server must be reconnected"
            );
            return None;
        }
    };
    let access_token = refreshed.access_token.trim().to_owned();
    if access_token.is_empty() {
        warn!(
            server_id,
            "mcp oauth: refresh returned an empty access token"
        );
        return None;
    }

    // RFC 6749 §6 lets the authorization server omit `refresh_token` and
    // `scope` on a refresh, meaning "keep what you had". capability-core's
    // upsert rewrites every column, so the stored values must be carried
    // forward explicitly — otherwise a rotating refresh token gets wiped and
    // the *next* refresh becomes impossible.
    let retained_refresh = refreshed
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(refresh_token);
    let scope = refreshed
        .scope
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(stored.scope.as_str());
    let payload = json!({
        "access_token": access_token,
        "refresh_token": retained_refresh,
        "token_type": stored.token_type,
        "scope": scope,
        "expires_in_seconds": refreshed.expires_in,
        "token_endpoint": token_endpoint,
        "client_id": stored.client_id,
    });
    if !persist_oauth_token(
        client,
        capability_core_base_url,
        service_token,
        org_id,
        server_id,
        &payload,
    )
    .await
    {
        // The refreshed token is still good for this call, so hand it back.
        // The cost of not persisting is a repeat refresh next time — and, if
        // this server rotates refresh tokens, a stored refresh token that is
        // now spent, which surfaces as an honest "reconnect required" rather
        // than a silent wrong answer.
        warn!(
            server_id,
            "mcp oauth: refreshed token could not be persisted; the next call will refresh again"
        );
    }
    Some(access_token)
}

/// Write refreshed tokens back through capability-core's internal upsert — the
/// service-token twin of the per-user `PUT /api/v1/mcp/{id}/oauth-tokens` that
/// the interactive connect flow uses, for the same reason the resolve above has
/// one: no per-user bearer exists on this path.
///
/// `service_token` is the root secret; the header carries the per-org
/// derivation, identically to the resolve path — see
/// `derive_org_scoped_service_token`. Both routes must be derived the same way
/// or a refresh write-back 403s while reads keep working.
async fn persist_oauth_token(
    client: &reqwest::Client,
    capability_core_base_url: &str,
    service_token: &str,
    org_id: &str,
    server_id: &str,
    payload: &serde_json::Value,
) -> bool {
    let Ok(mut url) = reqwest::Url::parse(&format!(
        "{capability_core_base_url}/api/v1/internal/mcp/oauth-tokens"
    )) else {
        return false;
    };
    url.query_pairs_mut()
        .append_pair("server_id", server_id)
        .append_pair("org_id", org_id);
    client
        .put(url)
        .header(
            MCP_SERVICE_TOKEN_HEADER,
            derive_org_scoped_service_token(service_token, org_id),
        )
        .json(payload)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
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
            "https://verevon.example.test/callback",
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
            "https://verevon.example.test/callback",
            "state-abc",
            "challenge-xyz",
            &[],
        );
        assert!(!url.contains("scope="));
    }

    // ---- per-org internal service token derivation -------------------------
    //
    // CROSS-LANGUAGE KNOWN-ANSWER VECTOR. The identical (secret, org_id,
    // expected hex) triple is asserted in capability-core's Go tests
    // (services/capability-core/internal/api/mcp_oauth_internal_test.go,
    // TestInternalServiceTokenDerivationMatchesTheGatewayKnownAnswerVector).
    // If either side's algorithm, encoding, or message framing drifts, one of
    // the two tests fails here rather than silently 403ing every MCP OAuth
    // token resolve in production.
    const VECTOR_SECRET: &str = "mcp-oauth-service-token-test-vector";
    const VECTOR_ORG: &str = "org-a";
    const VECTOR_EXPECTED: &str =
        "9b70d49c9cecb4ae9d5c871372409886a492d8dde023674d24053f916f6dfd8b";

    #[test]
    fn derived_service_token_matches_the_cross_language_known_answer_vector() {
        assert_eq!(
            derive_org_scoped_service_token(VECTOR_SECRET, VECTOR_ORG),
            VECTOR_EXPECTED
        );
    }

    #[test]
    fn derived_service_token_is_deterministic() {
        assert_eq!(
            derive_org_scoped_service_token(VECTOR_SECRET, VECTOR_ORG),
            derive_org_scoped_service_token(VECTOR_SECRET, VECTOR_ORG)
        );
    }

    #[test]
    fn derived_service_token_is_lowercase_hex_of_a_full_sha256() {
        let derived = derive_org_scoped_service_token(VECTOR_SECRET, VECTOR_ORG);
        assert_eq!(derived.len(), 64, "32-byte digest, two hex chars per byte");
        assert!(derived
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    // The whole point of the derivation: a token intercepted for one org is
    // useless against another.
    #[test]
    fn different_orgs_derive_different_tokens_under_the_same_secret() {
        let org_a = derive_org_scoped_service_token(VECTOR_SECRET, "org-a");
        let org_b = derive_org_scoped_service_token(VECTOR_SECRET, "org-b");
        assert_ne!(org_a, org_b);
    }

    #[test]
    fn different_secrets_derive_different_tokens_for_the_same_org() {
        assert_ne!(
            derive_org_scoped_service_token("root-secret-one", VECTOR_ORG),
            derive_org_scoped_service_token("root-secret-two", VECTOR_ORG)
        );
    }

    #[test]
    fn derived_service_token_never_leaks_the_root_secret() {
        let derived = derive_org_scoped_service_token(VECTOR_SECRET, VECTOR_ORG);
        assert_ne!(derived, VECTOR_SECRET);
        assert!(!derived.contains(VECTOR_SECRET));
    }

    // capability-core derives from the TrimSpace'd query parameter, so this
    // side must normalize identically or a padded org_id 403s.
    #[test]
    fn org_id_is_trimmed_before_hashing_like_capability_core_does() {
        assert_eq!(
            derive_org_scoped_service_token(VECTOR_SECRET, "  org-a\n"),
            VECTOR_EXPECTED
        );
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

    // Stronger than the two short-circuit tests above, which pass even if the
    // early return were removed (the host simply fails to resolve): a real
    // listener that must receive ZERO requests proves the empty-secret path
    // never touches the network.
    #[tokio::test]
    async fn empty_service_token_short_circuits_before_reaching_capability_core() {
        use wiremock::matchers::any;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let capability_core = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&capability_core)
            .await;

        let client = reqwest::Client::new();
        let token =
            resolve_stored_oauth_token(&client, &capability_core.uri(), "", "org-a", "srv-1").await;
        assert!(token.is_none());
        // MockServer verifies mounted expectations on drop.
    }

    // The wire assertion: what leaves the process is the per-org derivation,
    // never the root secret.
    #[tokio::test]
    async fn resolve_presents_the_org_derived_token_and_never_the_root_secret() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let capability_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/internal/mcp/oauth-token"))
            .and(query_param("server_id", "srv-1"))
            .and(query_param("org_id", VECTOR_ORG))
            .and(header(MCP_SERVICE_TOKEN_HEADER, VECTOR_EXPECTED))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "live-access",
                "refresh_token": "",
                "token_type": "Bearer",
                "scope": "",
                "token_endpoint": "",
                "client_id": ""
            })))
            .expect(1)
            .mount(&capability_core)
            .await;

        let client = reqwest::Client::new();
        let token = resolve_stored_oauth_token(
            &client,
            &capability_core.uri(),
            VECTOR_SECRET,
            VECTOR_ORG,
            "srv-1",
        )
        .await;
        assert_eq!(token.as_deref(), Some("live-access"));
    }

    // A token minted for another org must not be what we present — this mock
    // only answers the org-a derivation, so presenting org-b's would 404 and
    // resolve to None.
    #[tokio::test]
    async fn resolve_for_another_org_presents_a_different_token() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let capability_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/internal/mcp/oauth-token"))
            .and(header(MCP_SERVICE_TOKEN_HEADER, VECTOR_EXPECTED))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "org-a-only"
            })))
            .expect(0)
            .mount(&capability_core)
            .await;

        let client = reqwest::Client::new();
        let token = resolve_stored_oauth_token(
            &client,
            &capability_core.uri(),
            VECTOR_SECRET,
            "org-b",
            "srv-1",
        )
        .await;
        assert!(
            token.is_none(),
            "org-b must not be able to present org-a's derived token"
        );
    }

    fn expiry_now() -> DateTime<Utc> {
        "2026-07-28T20:37:26Z".parse().expect("fixed instant")
    }

    #[test]
    fn already_expired_token_is_not_live() {
        // The production incident verbatim: a token that expired at 20:19:03
        // was still handed to a 20:37:26 chat turn, which 401'd.
        assert!(!stored_access_token_is_live(
            Some("2026-07-28T20:19:03+00:00"),
            expiry_now()
        ));
    }

    #[test]
    fn token_expiring_inside_the_skew_is_not_live() {
        assert!(!stored_access_token_is_live(
            Some("2026-07-28T20:38:00Z"),
            expiry_now()
        ));
    }

    #[test]
    fn token_expiring_beyond_the_skew_is_live() {
        assert!(stored_access_token_is_live(
            Some("2026-07-28T20:38:30Z"),
            expiry_now()
        ));
    }

    #[test]
    fn absent_expiry_is_live() {
        // Nullable column: the authorization server never sent expires_in.
        assert!(stored_access_token_is_live(None, expiry_now()));
        assert!(stored_access_token_is_live(Some("  "), expiry_now()));
    }

    #[test]
    fn unparseable_expiry_is_live() {
        // No proof of death, so no refresh — see the fn's doc comment.
        assert!(stored_access_token_is_live(
            Some("28.07.2026 20:19"),
            expiry_now()
        ));
    }

    #[test]
    fn non_utc_expiry_offset_is_respected() {
        // 22:19:03+02:00 is 20:19:03Z — expired. A naive string/prefix compare
        // would read the hour as later than now and call it live.
        assert!(!stored_access_token_is_live(
            Some("2026-07-28T22:19:03+02:00"),
            expiry_now()
        ));
    }

    #[tokio::test]
    async fn expired_token_without_refresh_material_resolves_to_none() {
        let client = reqwest::Client::new();
        let stored = StoredOAuthToken {
            access_token: "expired-but-present".to_owned(),
            refresh_token: String::new(),
            token_type: "Bearer".to_owned(),
            scope: String::new(),
            expires_at: Some("2026-07-28T20:19:03+00:00".to_owned()),
            token_endpoint: String::new(),
            client_id: "client-123".to_owned(),
        };
        // No refresh_token and no token_endpoint: refusal is decided before any
        // network call, so this stays offline and deterministic.
        let token = refresh_stored_oauth_token(
            &client,
            "http://capability-core:8085",
            "shared-secret",
            "org-1",
            "srv-1",
            &stored,
        )
        .await;
        assert!(token.is_none());
    }
}
