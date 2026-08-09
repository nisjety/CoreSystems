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
//! - Accepts canonical Auth Core user and service principals. Service actors
//!   are derived only from signed `service_id`/`sub`, never request headers.
//!
//! Environment:
//! - `AUTH_CORE_JWKS_URL`             — required (unless dev bypass).
//! - `AUTH_CORE_ISSUER`               — optional; enforces `iss` when set.
//! - `AUTH_CORE_AUDIENCE`             — required by default; enforces `aud`.
//!   Set `AUTH_CORE_AUDIENCE_OPTIONAL=1` to opt out (warn-only mode).
//! - `AUTH_CORE_JWKS_TTL_SECS`        — optional; default 300s.
//! - `AUTH_CORE_JWT_LEEWAY_SECS`      — optional; default 30s. Clock-skew
//!   tolerance applied to `exp` and `nbf` validation.
//! - `QUARRY_EDGE_AUTH_DEV_BYPASS`    — `1`/`true` to accept a bearer that
//!   FAILS real verification and inject a stub `Claims` for local dev, so a
//!   legacy static/non-JWT caller can still reach protected routes. NEVER in
//!   production.
//!
//! Dev-bypass ordering (P0 fix — Aquatiq crawl-to-KB pipeline, 2026-07-20):
//! every bearer is run through the SAME real JWKS verification the strict
//! path uses, REGARDLESS of whether the bypass flag is set. Only when that
//! verification fails does the bypass flag get consulted, and only then does
//! the request fall back to the stub `Claims{org_id:"org_placeholder", ...}`.
//! Previously the bypass short-circuited BEFORE verification, so it silently
//! discarded every real, correctly-signed, real-org-scoped bearer the gateway
//! mints per user (`quarry_token()` → `aud=quarry`) and replaced it with the
//! placeholder identity — corrupting `org_id` end-to-end for every crawl/batch
//! handoff and any downstream Data Plane ingest keyed off it. Verifying first
//! means: a real, valid bearer always wins on its own merits; the bypass only
//! ever covers a bearer that could never have passed verification anyway
//! (e.g. a shared static dev token), so it can no longer clobber real tenant
//! identity while remaining available for that legitimate fallback case.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

/// Claims extracted from a verified JWT. Every token requires `org_id` plus
/// one unambiguous principal: user tokens carry `user_id`; service tokens carry
/// canonical `principal_type=service` + `service_id` equal to `sub`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub exp: i64,
    pub org_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
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

    /// True only for a canonical Auth Core service principal.
    #[must_use]
    pub fn is_service(&self) -> bool {
        self.principal_type.as_deref() == Some("service")
    }

    /// Canonical actor used by ownership/audit paths. `require_auth` normalizes
    /// service principals so this value is the signed `service_id`; user tokens
    /// retain their signed `user_id` unchanged.
    #[must_use]
    pub fn actor_id(&self) -> &str {
        &self.user_id
    }

    fn validate_and_normalize_principal(&mut self) -> Result<(), ()> {
        if self.org_id.trim().is_empty() {
            return Err(());
        }
        match self.principal_type.as_deref() {
            Some("service") => {
                let service_id = self.service_id.as_deref().unwrap_or("").trim();
                if service_id.is_empty()
                    || self.sub != service_id
                    || !self.user_id.trim().is_empty()
                    || !service_id.starts_with("service:")
                {
                    return Err(());
                }
                self.user_id = service_id.to_owned();
                Ok(())
            }
            Some("user") | None => {
                if self.user_id.trim().is_empty() || self.service_id.is_some() {
                    Err(())
                } else {
                    Ok(())
                }
            }
            Some(_) => Err(()),
        }
    }

    /// Enforces tenant isolation. Returns `Err(FORBIDDEN)` when the token's
    /// `org_id` does not match the expected value.
    ///
    /// # Errors
    /// [`StatusCode::FORBIDDEN`] on mismatch.
    #[allow(dead_code)] // scaffolding: wired in follow-up
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

/// `QUARRY_EDGE_AUTH_DEV_BYPASS` is read **once** at process startup and
/// cached for the lifetime of the binary. Two reasons:
///
/// 1. The startup kill switch in `main.rs` reads the same env var to
///    decide whether to refuse-to-start. If we re-read here per request
///    an operator could `docker exec` into a running container, flip
///    the env var, and silently enable the bypass without triggering
///    the startup guard. Latching ties both checks to the same value.
///
/// 2. Per-request `std::env::var` is a syscall; caching avoids it on
///    the auth hot path.
///
/// To toggle the bypass in dev: change the env var and restart the
/// service. Test builds bypass the latch so each test can flip the env
/// var independently.
fn dev_bypass_enabled() -> bool {
    #[cfg(test)]
    {
        read_dev_bypass_from_env()
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static CACHED: OnceLock<bool> = OnceLock::new();
        *CACHED.get_or_init(read_dev_bypass_from_env)
    }
}

fn read_dev_bypass_from_env() -> bool {
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

/// Verifies a bearer against the real Auth Core JWKS path: header decode,
/// algorithm pin, `kid` lookup (with one forced refresh on miss), signature +
/// `exp`/`nbf`/`iss`/`aud` validation, then principal normalization. This is
/// the strict path — used unconditionally, regardless of `QUARRY_EDGE_AUTH_DEV_BYPASS`,
/// so a real bearer is always verified for real before any bypass is consulted.
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] on header decode / algorithm / kid / signature
///   / claims-validation / principal-normalization failure.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched, the JWK
///   cannot be materialised into a decoding key, or `AUTH_CORE_AUDIENCE` is
///   unset without `AUTH_CORE_AUDIENCE_OPTIONAL=1`.
async fn verify_jwt_bearer(token: &str) -> Result<Claims, StatusCode> {
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
        // An empty AUTH_CORE_ISSUER must NOT enforce iss="" (mirrors the
        // model-gateway fix 672ed214) — skip issuer validation instead.
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

    let mut token_data = decode::<Claims>(token, &decoding_key, &validation).map_err(|e| {
        warn!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;

    if token_data
        .claims
        .validate_and_normalize_principal()
        .is_err()
    {
        warn!("JWT claims contain an invalid or ambiguous principal");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(token_data.claims)
}

/// The stub identity injected only when `QUARRY_EDGE_AUTH_DEV_BYPASS` is
/// enabled AND the presented bearer already failed real JWKS verification.
/// NEVER used for a bearer that verifies successfully — see [`require_auth`].
fn dev_bypass_stub_claims() -> Claims {
    Claims {
        sub: "user_placeholder".to_owned(),
        iss: "dev".to_owned(),
        exp: i64::MAX,
        org_id: "org_placeholder".to_owned(),
        user_id: "user_placeholder".to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
    }
}

/// Axum middleware that verifies a Bearer JWT and injects [`Claims`] into
/// the request's extension map. Handlers downstream of this middleware
/// can recover the claims with `Extension<Claims>`.
///
/// Every bearer is verified against the real Auth Core JWKS path FIRST,
/// regardless of `QUARRY_EDGE_AUTH_DEV_BYPASS`. Only when that verification
/// fails is the bypass flag consulted, and only then does the request fall
/// back to a stub `Claims{org_id:"org_placeholder", ...}` for local dev —
/// so the bypass can never silently discard a real, correctly-signed,
/// real-org-scoped bearer (see module docs for the incident this fixes).
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] when no usable token is present, or it
///   fails verification and the bypass is not enabled.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched or
///   the JWK cannot be materialised into a decoding key (and the bypass is
///   not enabled).
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

    match verify_jwt_bearer(&token).await {
        Ok(claims) => {
            req.extensions_mut().insert(claims);
            Ok(next.run(req).await)
        }
        Err(status) if dev_bypass_enabled() => {
            warn!(
                %status,
                "QUARRY_EDGE_AUTH_DEV_BYPASS enabled — bearer failed real JWKS \
                 verification, falling back to stub claims (org_id=org_placeholder)"
            );
            req.extensions_mut().insert(dev_bypass_stub_claims());
            Ok(next.run(req).await)
        }
        Err(status) => Err(status),
    }
}

/// Returns an Axum middleware that enforces a required scope on the
/// already-validated [`Claims`] in request extensions.
///
/// Must be layered *after* [`require_auth`]. Returns 401 if no `Claims`
/// extension is present (auth wasn't run) and 403 if the scope is missing.
#[allow(dead_code)]
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

/// Enforces least privilege for service principals while preserving existing
/// interactive-user behavior. Unknown service routes fail closed; each Model
/// Plane integration route declares the only scope(s) it accepts.
pub async fn require_service_route_scope(req: Request, next: Next) -> Result<Response, StatusCode> {
    let Some(claims) = req.extensions().get::<Claims>() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if !claims.is_service() {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    let permitted = match path {
        "/v1/scrape" => claims.has_scope("scrape:read") || claims.has_scope("scrape:write"),
        "/v1/search" => claims.has_scope("search:read"),
        "/v1/extract" => claims.has_scope("extract:read"),
        // Reading one artifact by id is part of the scrape/agent contract, not a
        // separate capability: `/v1/scrape` answers with
        // `FormatRef{artifact_id,bytes}` and never inline page text, and agent
        // observations hand back screenshot/trace ids. A principal allowed to
        // produce those refs but not to resolve them can only ever see empty
        // content. The bytes stay tenant-bound in `get_artifact`, which matches
        // the artifact's stored org against this claim.
        //
        // The `/v1/artifacts` *list* route is deliberately not included — no
        // service integration enumerates artifacts.
        _ if path.starts_with("/v1/artifacts/") => {
            claims.has_scope("scrape:read")
                || claims.has_scope("scrape:write")
                || claims.has_scope("browser:execute")
        }
        _ if path.starts_with("/v1/agent/") => claims.has_scope("browser:execute"),
        _ => false,
    };
    if permitted {
        Ok(next.run(req).await)
    } else {
        warn!(
            path,
            service = %claims.actor_id(),
            "service principal lacks the route scope"
        );
        Err(StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body, http::Request as HttpRequest, middleware, routing::get, Extension, Router,
    };
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
            principal_type: None,
            service_id: None,
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
            principal_type: None,
            service_id: None,
            nbf: None,
            aud: Some("quarry-edge".into()),
            scopes: vec!["search:read".into(), "crawl:write".into()],
        };
        assert!(c.has_scope("search:read"));
        assert!(!c.has_scope("admin"));
        assert!(c.require_org("alpha").is_ok());
        assert!(c.require_org("beta").is_err());
    }

    #[test]
    fn canonical_service_principal_derives_actor_from_signed_service_id() {
        let mut claims = Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: String::new(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
        };

        claims.validate_and_normalize_principal().unwrap();
        assert!(claims.is_service());
        assert_eq!(claims.actor_id(), "service:model-gateway");
    }

    #[test]
    fn canonical_service_payload_without_user_id_deserializes_and_normalizes() {
        let mut claims: Claims = serde_json::from_value(serde_json::json!({
            "sub": "service:execution-core",
            "iss": "auth-core",
            "exp": 1_900_000_000_i64,
            "org_id": "alpha",
            "principal_type": "service",
            "service_id": "service:execution-core",
            "aud": "quarry",
            "scopes": ["browser:execute"]
        }))
        .expect("Auth Core canonical service payload must deserialize");

        assert!(claims.user_id.is_empty());
        claims.validate_and_normalize_principal().unwrap();
        assert_eq!(claims.actor_id(), "service:execution-core");
    }

    #[test]
    fn ambiguous_service_principal_is_rejected() {
        let mut claims = Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "caller-selected-user".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
        };
        assert!(claims.validate_and_normalize_principal().is_err());
    }

    fn scoped_router(claims: Claims) -> Router {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        Router::new()
            .route("/v1/search", get(ok_handler))
            .route("/v1/extract", get(ok_handler))
            .route("/v1/other", get(ok_handler))
            .route("/v1/artifacts", get(ok_handler))
            .route("/v1/artifacts/:id", get(ok_handler))
            .layer(middleware::from_fn(require_service_route_scope))
            .layer(Extension(claims))
    }

    fn service_claims(scopes: &[&str]) -> Claims {
        Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    async fn status_for(claims: Claims, path: &str) -> StatusCode {
        let request = HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        tower::ServiceExt::oneshot(scoped_router(claims), request)
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn scrape_scoped_service_can_resolve_the_artifact_its_scrape_returned() {
        // `/v1/scrape` answers with `FormatRef{artifact_id,bytes}` and no inline
        // text, so denying the by-id read made every service fetch resolve to
        // empty content.
        for scope in ["scrape:read", "scrape:write", "browser:execute"] {
            assert_eq!(
                status_for(service_claims(&[scope]), "/v1/artifacts/art_01ABC").await,
                StatusCode::OK,
                "{scope} must be able to read an artifact by id"
            );
        }
    }

    #[tokio::test]
    async fn artifact_read_stays_closed_without_a_producing_scope() {
        assert_eq!(
            status_for(service_claims(&["search:read"]), "/v1/artifacts/art_01ABC").await,
            StatusCode::FORBIDDEN
        );
        // Enumerating artifacts is not part of any service integration, so the
        // collection route stays closed even for a scrape principal.
        assert_eq!(
            status_for(service_claims(&["scrape:read"]), "/v1/artifacts").await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn service_route_scope_is_least_privilege_and_unknown_routes_fail_closed() {
        let claims = Claims {
            sub: "service:model-gateway".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "service:model-gateway".into(),
            principal_type: Some("service".into()),
            service_id: Some("service:model-gateway".into()),
            nbf: None,
            aud: Some("quarry".into()),
            scopes: vec!["search:read".into()],
        };

        let search = HttpRequest::builder()
            .uri("/v1/search")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            tower::ServiceExt::oneshot(scoped_router(claims.clone()), search)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        for path in ["/v1/extract", "/v1/other"] {
            let request = HttpRequest::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                tower::ServiceExt::oneshot(scoped_router(claims.clone()), request)
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }

    #[tokio::test]
    async fn service_scope_middleware_preserves_interactive_user_routes() {
        let claims = Claims {
            sub: "user-1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "alpha".into(),
            user_id: "user-1".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: Some("quarry".into()),
            scopes: Vec::new(),
        };
        let request = HttpRequest::builder()
            .uri("/v1/other")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            tower::ServiceExt::oneshot(scoped_router(claims), request)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    /// Without an Authorization header, the middleware must return 401
    /// regardless of any other state. Tests are serialised because
    /// dev_bypass / JWKS env vars are process-global.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn missing_bearer_returns_401() {
        clear_env();
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let req = HttpRequest::builder().uri("/").body(Body::empty()).unwrap();
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

    // -----------------------------------------------------------------
    // P0 regression — Aquatiq crawl-to-KB pipeline (2026-07-20): dev-bypass
    // must verify a real bearer first and only ever stub when verification
    // genuinely fails, so it can never again clobber a real per-org JWT the
    // gateway mints (as it did in production, stamping every crawl job
    // "org_placeholder" instead of the real tenant).
    // -----------------------------------------------------------------

    async fn reset_jwks_cache_for_test() {
        *JWKS_CACHE.write().await = None;
    }

    fn echo_org_router() -> Router {
        async fn echo_org(Extension(claims): Extension<Claims>) -> String {
            claims.org_id.clone()
        }
        Router::new()
            .route("/", get(echo_org))
            .layer(middleware::from_fn(require_auth))
    }

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
            .and(path("/jwks"))
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

    /// The core regression: with the bypass ON, a real, validly-signed,
    /// real-org bearer must be verified and its REAL claims used — never
    /// silently replaced by the `org_placeholder` stub. This is the exact
    /// bug that stamped every Aquatiq crawl job with a placeholder org.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_prefers_valid_jwt_over_stub_claims() {
        clear_env();
        reset_jwks_cache_for_test().await;
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let kid = "test-kid-real";
        let server = start_jwks_mock(kid).await;
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{}/jwks", server.uri()));

        let claims = Claims {
            sub: "user-real".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-real-coresystem".into(),
            user_id: "user-real".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        let token = sign_jwt(&claims, kid);

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(echo_org_router(), req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"org-real-coresystem");

        clear_env();
        reset_jwks_cache_for_test().await;
    }

    /// Companion case: with the bypass ON, a bearer that genuinely fails
    /// verification (unknown kid — never issued by the configured JWKS)
    /// still falls back to the stub, preserving the legitimate local-dev
    /// escape hatch for non-JWT callers.
    #[tokio::test(flavor = "current_thread")]
    #[serial]
    async fn dev_bypass_falls_back_to_stub_when_jwt_verification_fails() {
        clear_env();
        reset_jwks_cache_for_test().await;
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");

        let kid = "test-kid-real-2";
        let server = start_jwks_mock(kid).await;
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{}/jwks", server.uri()));

        // Signed with a kid the JWKS mock never advertises — verification
        // must fail (no matching JWK even after a forced refresh).
        let claims = Claims {
            sub: "user-real".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-real-coresystem".into(),
            user_id: "user-real".into(),
            principal_type: Some("user".into()),
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        let token = sign_jwt(&claims, "unknown-kid");

        let req = HttpRequest::builder()
            .uri("/")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = tower::ServiceExt::oneshot(echo_org_router(), req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"org_placeholder");

        clear_env();
        reset_jwks_cache_for_test().await;
    }
}
