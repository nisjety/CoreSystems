//! Auth middleware — validates JWTs via JWKS fetched from `auth-core`.
//!
//! Behaviour:
//! - Extracts `Authorization: Bearer <token>` header.
//! - Fetches and TTL-caches the JWKS document from `AUTH_CORE_JWKS_URL`.
//! - Pins JWS algorithm to RS256 (blocks alg-confusion attacks).
//! - Verifies signature, `exp`, `iss` (optional), and `aud` (optional) claims.
//! - On unknown `kid`, forces a JWKS refresh (key rotation support).
//! - Injects the decoded [`Claims`] into request extensions for handlers.
//!
//! Environment:
//! - `AUTH_CORE_JWKS_URL`            — required (unless dev bypass).
//! - `AUTH_CORE_ISSUER`              — optional; enforces `iss` when set.
//! - `AUTH_CORE_AUDIENCE`            — required by default; enforces `aud`.
//!   Set `AUTH_CORE_AUDIENCE_OPTIONAL=1` to opt out (warn-only mode).
//! - `AUTH_CORE_JWKS_TTL_SECS`       — optional; default 300s.
//! - `AUTH_CORE_JWT_LEEWAY_SECS`     — optional; default 30s. Clock-skew
//!   tolerance applied to `exp` and `nbf` validation.
//! - `MODEL_GATEWAY_AUTH_DEV_BYPASS` — `1`/`true` to accept any bearer in local
//!   dev and derive `Claims` from trusted gateway `x-user-id` / `x-org-id`
//!   headers when present.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

/// Claims extracted from a verified JWT. Fields `org_id` and `user_id` are
/// required by downstream middleware (`rate_limit`) and handlers (`sse`).
///
/// `aud` and `scopes` are optional, additive extensions — absent tokens
/// deserialize with default values so existing issuers remain compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub exp: i64,
    pub org_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

impl Claims {
    /// Returns `true` if the token carries the given scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Enforces tenant isolation: returns `Err(FORBIDDEN)` when
    /// `self.org_id != expected`.
    ///
    /// # Errors
    /// [`StatusCode::FORBIDDEN`] on mismatch.
    pub fn require_org(&self, expected: &str) -> Result<(), StatusCode> {
        if self.org_id == expected {
            Ok(())
        } else {
            warn!(
                claim_org = %self.org_id,
                expected_org = %expected,
                "tenant mismatch"
            );
            Err(StatusCode::FORBIDDEN)
        }
    }
}

struct CachedJwks {
    fetched_at: Instant,
    jwks: JwkSet,
}

/// Module-level JWKS cache with TTL refresh. `None` = never populated.
static JWKS_CACHE: LazyLock<RwLock<Option<CachedJwks>>> = LazyLock::new(|| RwLock::new(None));

const DEFAULT_JWKS_TTL_SECS: u64 = 300;
const DEFAULT_JWT_LEEWAY_SECS: u64 = 30;

fn jwks_ttl() -> Duration {
    std::env::var("AUTH_CORE_JWKS_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or_else(
            || Duration::from_secs(DEFAULT_JWKS_TTL_SECS),
            Duration::from_secs,
        )
}

fn jwt_leeway_secs() -> u64 {
    std::env::var("AUTH_CORE_JWT_LEEWAY_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_JWT_LEEWAY_SECS)
}

fn audience_optional() -> bool {
    std::env::var("AUTH_CORE_AUDIENCE_OPTIONAL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

async fn fetch_jwks() -> Result<JwkSet, StatusCode> {
    let url = std::env::var("AUTH_CORE_JWKS_URL").map_err(|_| {
        error!("AUTH_CORE_JWKS_URL not set");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let resp = reqwest::get(&url).await.map_err(|e| {
        error!(error = %e, url = %url, "failed to fetch JWKS");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let jwks: JwkSet = resp.json().await.map_err(|e| {
        error!(error = %e, "failed to parse JWKS JSON");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(jwks)
}

async fn get_jwks(force_refresh: bool) -> Result<JwkSet, StatusCode> {
    if !force_refresh {
        let guard = JWKS_CACHE.read().await;
        if let Some(cached) = guard.as_ref() {
            if cached.fetched_at.elapsed() < jwks_ttl() {
                return Ok(cached.jwks.clone());
            }
        }
    }
    let jwks = fetch_jwks().await?;
    let mut guard = JWKS_CACHE.write().await;
    *guard = Some(CachedJwks {
        fetched_at: Instant::now(),
        jwks: jwks.clone(),
    });
    Ok(jwks)
}

#[cfg(test)]
async fn reset_jwks_cache_for_test() {
    let mut guard = JWKS_CACHE.write().await;
    *guard = None;
}

fn dev_bypass_enabled() -> bool {
    std::env::var("MODEL_GATEWAY_AUTH_DEV_BYPASS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn dev_bypass_claims(headers: &HeaderMap) -> Claims {
    let user_id = header_string(headers, "x-user-id", "user_placeholder");
    let org_id = header_string(headers, "x-org-id", "org_placeholder");
    Claims {
        sub: user_id.clone(),
        iss: "dev".to_owned(),
        exp: i64::MAX,
        org_id,
        user_id,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
    }
}

fn header_string(headers: &HeaderMap, name: &'static str, fallback: &'static str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

/// Axum middleware that verifies a Bearer JWT and injects [`Claims`].
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] when the `Authorization` header is missing,
///   malformed, or the token fails header decode / signature / claims
///   validation against the JWKS.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched or the
///   JWK cannot be materialised into a decoding key.
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(token) = header else {
        warn!("missing or invalid Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if dev_bypass_enabled() {
        warn!("MODEL_GATEWAY_AUTH_DEV_BYPASS enabled — accepting bearer without verification");
        let claims = dev_bypass_claims(req.headers());
        req.extensions_mut().insert(claims);
        return Ok(next.run(req).await);
    }

    let jwt_header = decode_header(token).map_err(|e| {
        warn!(error = %e, "failed to decode JWT header");
        StatusCode::UNAUTHORIZED
    })?;

    // Pin algorithm — blocks alg-confusion (e.g. `none`, HS256 against public key).
    if jwt_header.alg != Algorithm::RS256 {
        warn!(alg = ?jwt_header.alg, "unsupported JWT algorithm");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let kid = jwt_header.kid.ok_or_else(|| {
        warn!("JWT header missing kid");
        StatusCode::UNAUTHORIZED
    })?;

    // Two-pass JWK lookup: on kid miss, force a JWKS refresh to pick up
    // rotated keys, then try again.
    let jwks = get_jwks(false).await?;
    let jwk = if let Some(k) = jwks.find(&kid) {
        k.clone()
    } else {
        let refreshed = get_jwks(true).await?;
        refreshed.find(&kid).cloned().ok_or_else(|| {
            warn!(kid = %kid, "no JWK matches token kid after refresh");
            StatusCode::UNAUTHORIZED
        })?
    };

    let decoding_key = DecodingKey::from_jwk(&jwk).map_err(|e| {
        error!(error = %e, "failed to build decoding key from JWK");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = jwt_leeway_secs();
    if let Ok(expected_iss) = std::env::var("AUTH_CORE_ISSUER") {
        // Enforce `iss` only when a NON-EMPTY issuer is configured. A compose
        // default of `${AUTH_CORE_ISSUER:-}` yields an empty string (Ok("")),
        // which previously did set_issuer(&[""]) — rejecting every real token
        // with InvalidIssuer. Empty/unset means "don't enforce" (iss is optional).
        let expected_iss = expected_iss.trim().to_owned();
        if !expected_iss.is_empty() {
            validation.set_issuer(&[expected_iss]);
        }
    }
    match std::env::var("AUTH_CORE_AUDIENCE") {
        Ok(expected_aud) => {
            validation.set_audience(&[expected_aud]);
        }
        Err(_) if audience_optional() => {
            warn!(
                "AUTH_CORE_AUDIENCE unset and AUTH_CORE_AUDIENCE_OPTIONAL=1; \
                 skipping audience validation"
            );
            validation.validate_aud = false;
        }
        Err(_) => {
            error!(
                "AUTH_CORE_AUDIENCE not set; refusing requests. Set the var or \
                 opt out via AUTH_CORE_AUDIENCE_OPTIONAL=1"
            );
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let token_data = decode::<Claims>(token, &decoding_key, &validation).map_err(|e| {
        warn!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;

    if token_data.claims.org_id.is_empty() || token_data.claims.user_id.is_empty() {
        warn!("JWT claims missing org_id or user_id");
        return Err(StatusCode::UNAUTHORIZED);
    }

    req.extensions_mut().insert(token_data.claims);
    Ok(next.run(req).await)
}

/// Returns an Axum middleware that enforces a required scope on the
/// already-validated [`Claims`] in request extensions.
///
/// Must be layered *after* [`require_auth`]. Returns 401 if no `Claims`
/// extension is present (auth wasn't run) and 403 if the scope is missing.
///
/// # Example
/// ```ignore
/// Router::new()
///     .route("/admin", get(handler))
///     .layer(middleware::from_fn(require_scope("models:write")))
///     .layer(middleware::from_fn(require_auth));
/// ```
pub fn require_scope(
    scope: &'static str,
) -> impl Fn(
    Request,
    Next,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, StatusCode>> + Send>,
> + Clone
       + Send
       + Sync
       + 'static {
    move |req: Request, next: Next| {
        Box::pin(async move {
            let Some(claims) = req.extensions().get::<Claims>() else {
                warn!(scope = %scope, "require_scope invoked without Claims extension");
                return Err(StatusCode::UNAUTHORIZED);
            };
            if claims.has_scope(scope) {
                Ok(next.run(req).await)
            } else {
                warn!(
                    scope = %scope,
                    org = %claims.org_id,
                    user = %claims.user_id,
                    "missing required scope"
                );
                Err(StatusCode::FORBIDDEN)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest, middleware, routing::get, Router};
    use serial_test::serial;
    use tower::ServiceExt;

    /// Clear every env var this module reads so tests are deterministic.
    ///
    /// Tests using this are serialised via `#[serial]` so no concurrent
    /// thread observes the transient environment.
    fn clear_env() {
        std::env::remove_var("AUTH_CORE_JWKS_URL");
        std::env::remove_var("AUTH_CORE_ISSUER");
        std::env::remove_var("AUTH_CORE_AUDIENCE");
        std::env::remove_var("AUTH_CORE_AUDIENCE_OPTIONAL");
        std::env::remove_var("AUTH_CORE_JWKS_TTL_SECS");
        std::env::remove_var("AUTH_CORE_JWT_LEEWAY_SECS");
        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    }

    fn test_router() -> Router {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        Router::new()
            .route("/", get(ok_handler))
            .layer(middleware::from_fn(require_auth))
    }

    async fn send(req: HttpRequest<Body>) -> StatusCode {
        test_router().oneshot(req).await.unwrap().status()
    }

    #[test]
    fn claims_roundtrip_json_with_defaults() {
        let c = Claims {
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: 1_700_000_000,
            org_id: "o1".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        let s = serde_json::to_string(&c).unwrap();
        // Defaults should not be emitted when empty/None.
        assert!(!s.contains("\"aud\""));
        assert!(!s.contains("\"scopes\""));
        assert!(!s.contains("\"nbf\""));
        let back: Claims = serde_json::from_str(&s).unwrap();
        assert_eq!(back.org_id, "o1");
        assert_eq!(back.user_id, "u1");
        assert!(back.aud.is_none());
        assert!(back.nbf.is_none());
        assert!(back.scopes.is_empty());
    }

    #[test]
    fn claims_roundtrip_with_aud_and_scopes() {
        let c = Claims {
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: 1_700_000_000,
            org_id: "o1".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: Some("model-gateway".into()),
            scopes: vec!["models:read".into(), "models:invoke".into()],
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Claims = serde_json::from_str(&s).unwrap();
        assert_eq!(back.aud.as_deref(), Some("model-gateway"));
        assert!(back.has_scope("models:invoke"));
        assert!(!back.has_scope("models:admin"));
    }

    #[test]
    fn require_org_matches_and_rejects() {
        let c = Claims {
            sub: "u".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "org-a".into(),
            user_id: "u".into(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        assert!(c.require_org("org-a").is_ok());
        assert_eq!(c.require_org("org-b"), Err(StatusCode::FORBIDDEN));
    }

    #[tokio::test]
    #[serial]
    async fn missing_authorization_header_returns_401() {
        clear_env();
        let req = HttpRequest::builder().uri("/").body(Body::empty()).unwrap();
        assert_eq!(send(req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn invalid_bearer_scheme_returns_401() {
        clear_env();
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn empty_bearer_token_returns_401() {
        clear_env();
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer    ")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn malformed_jwt_returns_401() {
        clear_env();
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn header_decode_failure_returns_401_not_500() {
        clear_env();
        // Three segments that look like a JWT but have invalid base64 in the
        // header — should not be treated as a server error.
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer !!!.!!!.!!!")
            .body(Body::empty())
            .unwrap();
        let status = send(req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    #[serial]
    async fn dev_bypass_accepts_any_bearer() {
        async fn echo_claims(axum::Extension(claims): axum::Extension<Claims>) -> String {
            format!("{}:{}", claims.org_id, claims.user_id)
        }

        clear_env();
        // Serialised via #[serial]; cleared by clear_env on next test.
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");

        let app = Router::new()
            .route("/", get(echo_claims))
            .layer(middleware::from_fn(require_auth));

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer whatever")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"org_placeholder:user_placeholder");

        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    }

    #[tokio::test]
    #[serial]
    async fn dev_bypass_uses_forwarded_actor_headers_when_present() {
        async fn echo_claims(axum::Extension(claims): axum::Extension<Claims>) -> String {
            format!("{}:{}", claims.org_id, claims.user_id)
        }

        clear_env();
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");

        let app = Router::new()
            .route("/", get(echo_claims))
            .layer(middleware::from_fn(require_auth));

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer whatever")
            .header("x-org-id", "org-real")
            .header("x-user-id", "user-real")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"org-real:user-real");

        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    }

    #[tokio::test]
    #[serial]
    async fn jwks_cache_reset_helper_works() {
        reset_jwks_cache_for_test().await;
        let guard = JWKS_CACHE.read().await;
        assert!(guard.is_none());
    }

    // ---------------------------------------------------------------------
    // Test helpers: RSA keypair, JWT signing, JWKS mock server.
    //
    // The keypair is generated once per process via OnceLock to keep the
    // test suite fast (RSA-2048 generation is the dominant cost).
    // ---------------------------------------------------------------------

    fn rsa_keypair_pem() -> &'static (String, String) {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
        static KP: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
        KP.get_or_init(|| {
            let mut rng = rand::thread_rng();
            let priv_key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("rsa keygen");
            let pub_key = rsa::RsaPublicKey::from(&priv_key);
            let priv_pem = priv_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode priv pem")
                .to_string();
            let pub_pem = pub_key
                .to_public_key_pem(LineEnding::LF)
                .expect("encode pub pem");
            (priv_pem, pub_pem)
        })
    }

    fn jwk_n_e() -> (String, String) {
        use base64::Engine;
        use rsa::pkcs8::DecodePublicKey;
        use rsa::traits::PublicKeyParts;
        let (_, pub_pem) = rsa_keypair_pem();
        let pk = rsa::RsaPublicKey::from_public_key_pem(pub_pem).expect("decode pub pem");
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pk.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pk.e().to_bytes_be());
        (n, e)
    }

    fn sign_jwt(claims: &Claims, kid: &str) -> String {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let (priv_pem, _) = rsa_keypair_pem();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let key = EncodingKey::from_rsa_pem(priv_pem.as_bytes()).expect("load priv key");
        encode(&header, claims, &key).expect("encode jwt")
    }

    async fn start_jwks_mock(kid: &str) -> wiremock::MockServer {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let (n, e) = jwk_n_e();
        let body = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": kid,
                "n": n,
                "e": e,
            }]
        });
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    fn now_secs() -> i64 {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_secs(),
        )
        .expect("unix epoch fits in i64")
    }

    fn base_claims() -> Claims {
        Claims {
            sub: "u".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-test".into(),
            user_id: "user-test".into(),
            nbf: None,
            aud: Some("model-gateway".into()),
            scopes: Vec::new(),
        }
    }

    async fn send_with_token(token: &str) -> StatusCode {
        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        send(req).await
    }

    // ---------------------------------------------------------------------
    // Gap 8 hardening tests: nbf, leeway, audience-default-required, scope.
    // ---------------------------------------------------------------------

    #[tokio::test]
    #[serial]
    async fn nbf_in_future_is_rejected() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-nbf";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");

        let mut claims = base_claims();
        // 5 minutes in the future, well outside the default 30s leeway.
        claims.nbf = Some(now_secs() + 300);
        let token = sign_jwt(&claims, kid);

        assert_eq!(send_with_token(&token).await, StatusCode::UNAUTHORIZED);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn leeway_boundary_accepts_recent_nbf() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-leeway";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        // Allow generous leeway so a near-future nbf still validates.
        std::env::set_var("AUTH_CORE_JWT_LEEWAY_SECS", "120");

        let mut claims = base_claims();
        claims.nbf = Some(now_secs() + 30);
        let token = sign_jwt(&claims, kid);

        assert_eq!(send_with_token(&token).await, StatusCode::OK);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn missing_audience_env_returns_500_unless_opt_out() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-aud";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        // AUTH_CORE_AUDIENCE intentionally unset.

        let mut claims = base_claims();
        // Token still carries an aud; middleware must refuse to validate.
        claims.aud = Some("model-gateway".into());
        let token = sign_jwt(&claims, kid);

        assert_eq!(
            send_with_token(&token).await,
            StatusCode::INTERNAL_SERVER_ERROR,
        );

        // Opting out via AUTH_CORE_AUDIENCE_OPTIONAL accepts the same token.
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");
        assert_eq!(send_with_token(&token).await, StatusCode::OK);

        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn require_scope_returns_403_without_scope_and_200_with_scope() {
        async fn ok_handler() -> &'static str {
            "ok"
        }

        fn build_app(claims: Claims) -> Router {
            // Layer order: extensions inserted last run first, so the Claims
            // extension is present when require_scope inspects the request.
            Router::new()
                .route("/", get(ok_handler))
                .layer(middleware::from_fn(require_scope("admin")))
                .layer(axum::Extension(claims))
        }

        clear_env();

        // No scope → 403.
        let mut without_scope = base_claims();
        without_scope.scopes = vec!["models:read".into()];
        let resp = build_app(without_scope)
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // With scope → 200.
        let mut with_scope = base_claims();
        with_scope.scopes = vec!["admin".into()];
        let resp = build_app(with_scope)
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
