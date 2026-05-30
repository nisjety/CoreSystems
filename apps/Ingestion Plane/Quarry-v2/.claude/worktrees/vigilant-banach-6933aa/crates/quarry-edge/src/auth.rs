//! Auth middleware — validates JWTs via JWKS fetched from `auth-core`.
//!
//! P0 / cluster #auth+tenancy. Adapted from the canonical Model Plane
//! gateway implementation at
//! `apps/Model Plane/rust/services/model-gateway/src/auth.rs`. The same
//! Control Plane `auth-core` issues every JWT in the system, so Quarry
//! uses the identical JWKS verification path.
//!
//! Behaviour:
//! - Extracts `Authorization: Bearer <token>` header.
//! - Fetches and TTL-caches the JWKS document from `AUTH_CORE_JWKS_URL`.
//! - Pins JWS algorithm to RS256 (blocks `alg=none` and HS256-against-public-key).
//! - Verifies signature + `exp` + `nbf` + optional `iss` + optional `aud`.
//! - On unknown `kid`, forces a JWKS refresh (key rotation support).
//! - Injects the decoded [`Claims`] into request extensions for handlers.
//! - Rejects tokens whose claims are missing `org_id` or `user_id` — those
//!   two fields drive every downstream tenant-isolation decision.
//!
//! Environment:
//! - `AUTH_CORE_JWKS_URL`             — required (unless dev bypass).
//! - `AUTH_CORE_ISSUER`               — optional; enforces `iss` when set.
//! - `AUTH_CORE_AUDIENCE`             — required by default; enforces `aud`.
//!   Set `AUTH_CORE_AUDIENCE_OPTIONAL=1` to opt out (warn-only mode).
//! - `AUTH_CORE_JWKS_TTL_SECS`        — optional; default 300s.
//! - `AUTH_CORE_JWT_LEEWAY_SECS`      — optional; default 30s. Clock-skew
//!   tolerance applied to `exp` and `nbf` validation.
//! - `QUARRY_EDGE_AUTH_DEV_BYPASS`    — `1`/`true` to accept any bearer
//!   and inject a stub `Claims` for local dev. NEVER in production.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

/// Claims extracted from a verified JWT. `org_id` and `user_id` are
/// required — they drive every downstream tenant-isolation decision in
/// handlers and the Tantivy local index. `aud` and `scopes` are optional
/// additive extensions; tokens without them deserialize with defaults.
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
    /// Does the token carry the given scope?
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Enforces tenant isolation. Returns `Err(FORBIDDEN)` when the token's
    /// `org_id` does not match the expected value.
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

fn dev_bypass_enabled() -> bool {
    std::env::var("QUARRY_EDGE_AUTH_DEV_BYPASS")
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

/// Axum middleware that verifies a Bearer JWT and injects [`Claims`] into
/// the request's extension map. Handlers downstream of this middleware
/// can recover the claims with `Extension<Claims>`.
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] when no usable token is present, or it
///   fails header decode / signature / claims validation against the JWKS.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched or
///   the JWK cannot be materialised into a decoding key.
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let Some(token) = token else {
        warn!("missing or invalid Authorization bearer");
        return Err(StatusCode::UNAUTHORIZED);
    };
    let token = token.as_str();

    if dev_bypass_enabled() {
        warn!("QUARRY_EDGE_AUTH_DEV_BYPASS enabled — accepting bearer without verification");
        let claims = Claims {
            sub: "user_placeholder".to_owned(),
            iss: "dev".to_owned(),
            exp: i64::MAX,
            org_id: "org_placeholder".to_owned(),
            user_id: "user_placeholder".to_owned(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        req.extensions_mut().insert(claims);
        return Ok(next.run(req).await);
    }

    let jwt_header = decode_header(token).map_err(|e| {
        warn!(error = %e, "failed to decode JWT header");
        StatusCode::UNAUTHORIZED
    })?;

    // Pin algorithm — blocks alg-confusion (`none`, HS256 against public key).
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
        validation.set_issuer(&[expected_iss]);
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

    fn clear_env() {
        std::env::remove_var("AUTH_CORE_JWKS_URL");
        std::env::remove_var("AUTH_CORE_ISSUER");
        std::env::remove_var("AUTH_CORE_AUDIENCE");
        std::env::remove_var("AUTH_CORE_AUDIENCE_OPTIONAL");
        std::env::remove_var("AUTH_CORE_JWKS_TTL_SECS");
        std::env::remove_var("AUTH_CORE_JWT_LEEWAY_SECS");
        std::env::remove_var("QUARRY_EDGE_AUTH_DEV_BYPASS");
    }

    fn dev_router() -> Router {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        Router::new()
            .route("/", get(ok_handler))
            .layer(middleware::from_fn(require_auth))
    }

    #[test]
    fn claims_serde_roundtrip_default_fields_omitted() {
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
        assert!(!s.contains("\"aud\""));
        assert!(!s.contains("\"scopes\""));
        assert!(!s.contains("\"nbf\""));
        let back: Claims = serde_json::from_str(&s).unwrap();
        assert_eq!(back.org_id, "o1");
        assert_eq!(back.user_id, "u1");
        assert!(back.scopes.is_empty());
    }

    #[test]
    fn claims_has_scope_and_require_org() {
        let c = Claims {
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: Some("quarry-edge".into()),
            scopes: vec!["search:read".into(), "crawl:write".into()],
        };
        assert!(c.has_scope("search:read"));
        assert!(!c.has_scope("admin"));
        assert!(c.require_org("alpha").is_ok());
        assert!(c.require_org("beta").is_err());
    }

    /// Without an Authorization header, the middleware must return 401
    /// regardless of any other state. Tests are serialised because
    /// dev_bypass / JWKS env vars are process-global.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn missing_bearer_returns_401() {
        clear_env();
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let req = HttpRequest::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        clear_env();
    }

    /// Dev bypass injects a stub Claims so handlers see a deterministic
    /// org_id/user_id even when no real JWT is present.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_accepts_any_bearer() {
        clear_env();
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer anything")
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        clear_env();
    }

    /// Malformed token returns 401 even with audience optional + no JWKS
    /// configured — the JWT header decode runs before any JWKS fetch.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn malformed_token_returns_401() {
        clear_env();
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");
        // No AUTH_CORE_JWKS_URL — we should never reach the JWKS fetch
        // because the header-decode step fails first.

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(dev_router(), req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        clear_env();
    }
}
