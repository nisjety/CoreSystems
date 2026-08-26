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
//! - `AUTH_CORE_ISSUER`              — required; enforces `iss`.
//! - `AUTH_CORE_AUDIENCE`            — required; enforces `aud`.
//! - `AUTH_CORE_JWKS_TTL_SECS`       — optional; default 300s.
//! - `AUTH_CORE_JWT_LEEWAY_SECS`     — optional; default 30s. Clock-skew
//!   tolerance applied to `exp` and `nbf` validation.
//! - `MODEL_GATEWAY_AUTH_DEV_BYPASS` — accepts an unverified bearer only when
//!   this and `ALLOW_INSECURE_DEV_DEFAULTS` are both explicitly enabled.

use std::fmt;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{FromRequestParts, Request},
    http::{request::Parts, HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

/// Claims extracted from a verified JWT. Users carry `user_id`; services carry
/// a matching `sub`/`service_id`, bounded reason, and no user identity.
///
/// `aud`, `scopes`, and `zdr` are additive extensions. Identity validation below
/// rejects missing or ambiguous principal fields after signature verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub exp: i64,
    pub org_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// Restrictive zero-retention posture asserted by the verified issuer.
    /// Request bodies may make this stricter, but can never turn it off.
    #[serde(default)]
    pub zdr: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalKind {
    User,
    Service,
}

/// A distinct `aud=data-plane` user bearer, independently verified against the
/// Control JWKS and bound to the same subject and tenant as the Model token.
/// It is never accepted from caller-selected org/user headers.
#[derive(Clone)]
pub struct VerifiedDataPlaneBearer(Arc<str>);

impl VerifiedDataPlaneBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// A distinct `aud=ingestion` user bearer, independently verified against the
/// Control JWKS and bound to the same subject and tenant as the Model token.
/// Used only to call shipping-core (and any future Ingestion Plane HTTP API)
/// on the caller's behalf — never accepted from caller-selected headers.
#[derive(Clone)]
pub struct VerifiedIngestionBearer(Arc<str>);

impl VerifiedIngestionBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Marker proving that the public Model Gateway bearer passed verification.
/// The ingress credential is deliberately not retained: every downstream
/// boundary uses its own independently verified audience token.
#[derive(Clone)]
pub struct VerifiedModelBearer;

impl VerifiedModelBearer {
    fn new(_token: &str) -> Self {
        Self
    }

    #[cfg(test)]
    pub(crate) fn for_test(_token: &str) -> Self {
        Self
    }
}

/// Independently verified `aud=session-core` user bearer. It is accepted only
/// from `x-session-authorization` and must match the verified ingress token's
/// subject, user, and tenant. Model/Data credentials are never reused.
#[derive(Clone)]
pub struct VerifiedSessionBearer(Arc<str>);

impl VerifiedSessionBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// Independently verified `aud=inference-core` user bearer. It is accepted
/// only from `x-inference-authorization` and must match the verified ingress
/// token's subject, user, and tenant. No ingress or cross-plane token is reused.
#[derive(Clone)]
pub struct VerifiedInferenceBearer(Arc<str>);

impl VerifiedInferenceBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// Independently verified `aud=execution-core` user bearer. Model Gateway uses
/// it only as Execution Core ingress authority; Data Plane access remains a
/// separate delegated credential.
#[derive(Clone)]
pub struct VerifiedExecutionBearer(Arc<str>);

impl VerifiedExecutionBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// Independently verified `aud=browser-broker` user bearer. It is accepted
/// only from `x-browser-authorization`, bound to the ingress user and tenant,
/// and used exclusively to resolve a broker-issued browser grant.
#[derive(Clone)]
pub struct VerifiedBrowserBearer(Arc<str>);

impl VerifiedBrowserBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// Independently verified, user-bound bearer for capability-core. A Model
/// Plane token is never reused across audiences.
#[derive(Clone)]
pub struct VerifiedCapabilityBearer(Arc<str>);

impl VerifiedCapabilityBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

/// Independently verified, user-bound bearer for cost-core.
#[derive(Clone)]
pub struct VerifiedCostBearer(Arc<str>);

impl VerifiedCostBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for VerifiedCostBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedCostBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedCostBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

impl fmt::Debug for VerifiedCapabilityBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedCapabilityBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedCapabilityBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

impl fmt::Debug for VerifiedModelBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedModelBearer([REDACTED])")
    }
}

impl fmt::Debug for VerifiedSessionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedSessionBearer([REDACTED])")
    }
}

impl fmt::Debug for VerifiedInferenceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedInferenceBearer([REDACTED])")
    }
}

impl fmt::Debug for VerifiedExecutionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedExecutionBearer([REDACTED])")
    }
}

impl fmt::Debug for VerifiedBrowserBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedBrowserBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedExecutionBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedBrowserBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedInferenceBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedSessionBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedModelBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

impl fmt::Debug for VerifiedDataPlaneBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedDataPlaneBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedDataPlaneBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

impl fmt::Debug for VerifiedIngestionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedIngestionBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedIngestionBearer
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

impl Claims {
    fn principal_kind(&self) -> Result<PrincipalKind, StatusCode> {
        match self.principal_type.as_deref().unwrap_or("user") {
            "user"
                if !self.user_id.is_empty()
                    && self.sub == self.user_id
                    && self.service_id.is_none() =>
            {
                Ok(PrincipalKind::User)
            }
            "service"
                if self.user_id.is_empty()
                    && self.service_id.as_deref().is_some_and(|service_id| {
                        service_id == self.sub
                            && service_id
                                .strip_prefix("service:")
                                .is_some_and(|id| !id.is_empty())
                    })
                    && self.reason.as_deref().is_some_and(|reason| {
                        reason == reason.trim() && (3..=500).contains(&reason.len())
                    }) =>
            {
                Ok(PrincipalKind::Service)
            }
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }

    /// Returns `true` if the token carries the given scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Combine issuer-asserted and request-selected ZDR monotonically.
    #[must_use]
    pub const fn effective_zdr(&self, request_zdr: bool) -> bool {
        self.zdr || request_zdr
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
const MAX_JWKS_BYTES: usize = 1_048_576;

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

async fn fetch_jwks() -> Result<JwkSet, StatusCode> {
    let url = std::env::var("AUTH_CORE_JWKS_URL").map_err(|_| {
        error!("AUTH_CORE_JWKS_URL not set");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| {
            error!(%error, "failed to build bounded JWKS client");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut resp = client.get(&url).send().await.map_err(|e| {
        error!(error = %e, url = %url, "failed to fetch JWKS");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if !resp.status().is_success()
        || resp
            .content_length()
            .is_some_and(|length| length > MAX_JWKS_BYTES as u64)
    {
        error!(status = %resp.status(), "Auth Core JWKS response rejected");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|error| {
        error!(%error, "failed while reading Auth Core JWKS response");
        StatusCode::INTERNAL_SERVER_ERROR
    })? {
        if body.len().saturating_add(chunk.len()) > MAX_JWKS_BYTES {
            error!("Auth Core JWKS response exceeded the configured bound");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        body.extend_from_slice(&chunk);
    }
    let jwks: JwkSet = serde_json::from_slice(&body).map_err(|e| {
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
    // `validate_startup` requires the second insecure-development gate before
    // the executable opens a listener. Keeping this helper single-purpose
    // also lets middleware unit tests exercise the bypass without booting the
    // entire service.
    std::env::var("MODEL_GATEWAY_AUTH_DEV_BYPASS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn validate_startup_values(
    jwks_url: Option<&str>,
    issuer: Option<&str>,
    audience: Option<&str>,
    bypass_requested: bool,
    insecure_development: bool,
) -> Result<(), &'static str> {
    if bypass_requested {
        return if insecure_development {
            Ok(())
        } else {
            Err("MODEL_GATEWAY_AUTH_DEV_BYPASS requires ALLOW_INSECURE_DEV_DEFAULTS")
        };
    }
    let configured = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    if !configured(jwks_url) {
        return Err("AUTH_CORE_JWKS_URL is required");
    }
    if !configured(issuer) {
        return Err("AUTH_CORE_ISSUER is required");
    }
    if !configured(audience) {
        return Err("AUTH_CORE_AUDIENCE is required");
    }
    Ok(())
}

/// Validate authentication configuration before either public listener starts.
/// Production posture has no observe/optional verification mode.
///
/// # Errors
///
/// Returns an error when verification material is incomplete or when the
/// unverified development bypass is requested without its second explicit gate.
pub fn validate_startup() -> anyhow::Result<()> {
    let bypass_requested = std::env::var("MODEL_GATEWAY_AUTH_DEV_BYPASS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let insecure_development = std::env::var("ALLOW_INSECURE_DEV_DEFAULTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    validate_startup_values(
        std::env::var("AUTH_CORE_JWKS_URL").ok().as_deref(),
        std::env::var("AUTH_CORE_ISSUER").ok().as_deref(),
        std::env::var("AUTH_CORE_AUDIENCE").ok().as_deref(),
        bypass_requested,
        insecure_development,
    )
    .map_err(anyhow::Error::msg)
}

/// Fetch and validate the initial JWKS before either public listener starts.
/// Development bypass remains explicitly gated by [`validate_startup`] and
/// does not contact Auth Core.
///
/// # Errors
/// Returns an error when verification material is unavailable, redirected,
/// oversized, or malformed.
pub async fn warm_jwks() -> anyhow::Result<()> {
    if dev_bypass_enabled() {
        return Ok(());
    }
    get_jwks(true)
        .await
        .map(|_| ())
        .map_err(|status| anyhow::anyhow!("initial Auth Core JWKS fetch failed: {status}"))
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
        zdr: false,
        principal_type: Some("user".to_owned()),
        service_id: None,
        reason: None,
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

async fn verify_delegated_data_plane_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedDataPlaneBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-data-plane-authorization",
        "DATA_PLANE_AUTH_AUDIENCE",
        "data-plane",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedDataPlaneBearer::new(&token)))
}

async fn verify_delegated_ingestion_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedIngestionBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-ingestion-authorization",
        "INGESTION_AUTH_AUDIENCE",
        "ingestion",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedIngestionBearer::new(&token)))
}

async fn verify_delegated_capability_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedCapabilityBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-capability-authorization",
        "CAPABILITY_CORE_AUTH_AUDIENCE",
        "capability-core",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedCapabilityBearer::new(&token)))
}

async fn verify_delegated_cost_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedCostBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-cost-authorization",
        "COST_CORE_AUTH_AUDIENCE",
        "cost-core",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedCostBearer::new(&token)))
}

async fn verify_delegated_session_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedSessionBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-session-authorization",
        "SESSION_CORE_AUTH_AUDIENCE",
        "session-core",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedSessionBearer::new(&token)))
}

const SERVICE_SCOPE_SESSION_SPACE_DELETE: &str = "session:space-delete";

/// Service credentials normally cannot delegate into Session Core: a service
/// must never turn a gateway route into user impersonation. The sole exception
/// is the separately scoped Space deletion coordinator path, which still
/// requires an exact service identity and a second route scope below.
async fn verify_delegated_service_session_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedSessionBearer>, StatusCode> {
    let Some((token, claims)) = decode_delegated_bearer(
        headers,
        "x-session-authorization",
        "SESSION_CORE_AUTH_AUDIENCE",
        "session-core",
    )
    .await?
    else {
        return Ok(None);
    };
    if claims.principal_kind() != Ok(PrincipalKind::Service)
        || claims.sub != model_claims.sub
        || claims.service_id != model_claims.service_id
        || claims.org_id != model_claims.org_id
        || !claims.has_scope(SERVICE_SCOPE_SESSION_SPACE_DELETE)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(VerifiedSessionBearer::new(&token)))
}

async fn verify_delegated_inference_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedInferenceBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-inference-authorization",
        "INFERENCE_CORE_AUTH_AUDIENCE",
        "inference-core",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedInferenceBearer::new(&token)))
}

async fn verify_delegated_execution_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedExecutionBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-execution-authorization",
        "EXECUTION_CORE_AUTH_AUDIENCE",
        "execution-core",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedExecutionBearer::new(&token)))
}

async fn verify_delegated_browser_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedBrowserBearer>, StatusCode> {
    verify_delegated_user_bearer(
        headers,
        "x-browser-authorization",
        "BROWSER_BROKER_AUTH_AUDIENCE",
        "browser-broker",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedBrowserBearer::new(&token)))
}

async fn verify_delegated_user_bearer(
    headers: &HeaderMap,
    header_name: &'static str,
    audience_env: &'static str,
    default_audience: &'static str,
    model_claims: &Claims,
) -> Result<Option<String>, StatusCode> {
    let Some((token, claims)) =
        decode_delegated_bearer(headers, header_name, audience_env, default_audience).await?
    else {
        return Ok(None);
    };
    if claims.principal_kind() != Ok(PrincipalKind::User)
        || claims.sub != model_claims.sub
        || claims.user_id != model_claims.user_id
        || claims.org_id != model_claims.org_id
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(token))
}

async fn verify_delegated_service_inference_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedInferenceBearer>, StatusCode> {
    let Some((token, claims)) = decode_delegated_bearer(
        headers,
        "x-inference-authorization",
        "INFERENCE_CORE_AUTH_AUDIENCE",
        "inference-core",
    )
    .await?
    else {
        return Ok(None);
    };
    if claims.principal_kind() != Ok(PrincipalKind::Service)
        || claims.sub != model_claims.sub
        || claims.service_id != model_claims.service_id
        || claims.org_id != model_claims.org_id
        || !claims.has_scope(SERVICE_SCOPE_MODELS_INVOKE)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(VerifiedInferenceBearer::new(&token)))
}

async fn decode_delegated_bearer(
    headers: &HeaderMap,
    header_name: &'static str,
    audience_env: &'static str,
    default_audience: &'static str,
) -> Result<Option<(String, Claims)>, StatusCode> {
    let Some(raw) = headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };
    let token = raw
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let jwt_header = decode_header(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    if jwt_header.alg != Algorithm::RS256 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let kid = jwt_header.kid.ok_or(StatusCode::UNAUTHORIZED)?;
    let jwks = get_jwks(false).await?;
    let jwk = if let Some(key) = jwks.find(&kid) {
        key.clone()
    } else {
        get_jwks(true)
            .await?
            .find(&kid)
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)?
    };
    let key = DecodingKey::from_jwk(&jwk).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let issuer = std::env::var("AUTH_CORE_ISSUER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let audience = std::env::var(audience_env).unwrap_or_else(|_| default_audience.to_owned());
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = jwt_leeway_secs();
    let claims = decode::<Claims>(token, &key, &validation)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .claims;
    Ok(Some((token.to_owned(), claims)))
}

/// Axum middleware that verifies a Bearer JWT and injects [`Claims`].
///
/// # Errors
/// - [`StatusCode::UNAUTHORIZED`] when the `Authorization` header is missing,
///   malformed, or the token fails header decode / signature / claims
///   validation against the JWKS.
/// - [`StatusCode::INTERNAL_SERVER_ERROR`] when JWKS cannot be fetched or the
///   JWK cannot be materialised into a decoding key.
#[allow(clippy::too_many_lines)]
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let Some(token) = header else {
        warn!("missing or invalid Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if dev_bypass_enabled() {
        warn!("MODEL_GATEWAY_AUTH_DEV_BYPASS enabled — accepting bearer without verification");
        let claims = dev_bypass_claims(req.headers());
        let data_plane_bearer = verify_delegated_data_plane_bearer(req.headers(), &claims).await?;
        let capability_bearer = verify_delegated_capability_bearer(req.headers(), &claims).await?;
        let cost_bearer = verify_delegated_cost_bearer(req.headers(), &claims).await?;
        let session_bearer = verify_delegated_session_bearer(req.headers(), &claims).await?;
        let inference_bearer = verify_delegated_inference_bearer(req.headers(), &claims).await?;
        let execution_bearer = verify_delegated_execution_bearer(req.headers(), &claims).await?;
        let browser_bearer = verify_delegated_browser_bearer(req.headers(), &claims).await?;
        let ingestion_bearer = verify_delegated_ingestion_bearer(req.headers(), &claims).await?;
        req.extensions_mut().insert(claims);
        req.extensions_mut()
            .insert(VerifiedModelBearer::new(&token));
        // Delegated bearers are passed through ONLY when the caller supplied
        // and they verified. The bypass deliberately cannot MINT one — see
        // `dev_bypass_cannot_supply_a_data_plane_bearer`. A gateway dev flag
        // must not become cross-plane authority: a synthesised Data Plane or
        // Session bearer would turn "skip auth on my local gateway" into
        // unverified access to another plane's content. The consequence is
        // intentional and worth stating, because it looks like a bug from
        // outside: under the bypass, any route whose handler EXTRACTS a
        // delegated bearer still returns 401, so the bypass alone cannot
        // exercise those routes. That is the boundary working, not failing.
        if let Some(data_plane_bearer) = data_plane_bearer {
            req.extensions_mut().insert(data_plane_bearer);
        }
        if let Some(capability_bearer) = capability_bearer {
            req.extensions_mut().insert(capability_bearer);
        }
        if let Some(cost_bearer) = cost_bearer {
            req.extensions_mut().insert(cost_bearer);
        }
        if let Some(session_bearer) = session_bearer {
            req.extensions_mut().insert(session_bearer);
        }
        if let Some(inference_bearer) = inference_bearer {
            req.extensions_mut().insert(inference_bearer);
        }
        if let Some(execution_bearer) = execution_bearer {
            req.extensions_mut().insert(execution_bearer);
        }
        if let Some(browser_bearer) = browser_bearer {
            req.extensions_mut().insert(browser_bearer);
        }
        if let Some(ingestion_bearer) = ingestion_bearer {
            req.extensions_mut().insert(ingestion_bearer);
        }
        return Ok(next.run(req).await);
    }

    let jwt_header = decode_header(&token).map_err(|e| {
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
    let expected_iss = std::env::var("AUTH_CORE_ISSUER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error!("AUTH_CORE_ISSUER not set; refusing requests");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    validation.set_issuer(&[expected_iss]);
    let expected_aud = std::env::var("AUTH_CORE_AUDIENCE")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error!("AUTH_CORE_AUDIENCE not set; refusing requests");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    validation.set_audience(&[expected_aud]);

    let token_data = decode::<Claims>(&token, &decoding_key, &validation).map_err(|e| {
        warn!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;

    if token_data.claims.org_id.is_empty() {
        warn!("JWT claims missing org_id");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let principal_kind = token_data.claims.principal_kind().inspect_err(|_| {
        warn!("JWT carries missing, mismatched, or ambiguous principal identity");
    })?;
    let (
        data_plane_bearer,
        capability_bearer,
        cost_bearer,
        session_bearer,
        inference_bearer,
        execution_bearer,
        browser_bearer,
        ingestion_bearer,
    ) = match principal_kind {
        PrincipalKind::User => (
            verify_delegated_data_plane_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_capability_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_cost_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_session_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_inference_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_execution_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_browser_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_ingestion_bearer(req.headers(), &token_data.claims).await?,
        ),
        PrincipalKind::Service => {
            if req.headers().contains_key("x-data-plane-authorization")
                || req.headers().contains_key("x-capability-authorization")
                || req.headers().contains_key("x-cost-authorization")
                || req.headers().contains_key("x-execution-authorization")
                || req.headers().contains_key("x-browser-authorization")
                || req.headers().contains_key("x-ingestion-authorization")
                || (req.headers().contains_key("x-session-authorization")
                    && req.headers().contains_key("x-inference-authorization"))
            {
                warn!(
                    "service principal supplied an unauthorized or ambiguous downstream delegation"
                );
                return Err(StatusCode::FORBIDDEN);
            }
            let inference_bearer =
                verify_delegated_service_inference_bearer(req.headers(), &token_data.claims)
                    .await?;
            let session_bearer =
                verify_delegated_service_session_bearer(req.headers(), &token_data.claims).await?;
            (
                None,
                None,
                None,
                session_bearer,
                inference_bearer,
                None,
                None,
                None,
            )
        }
    };
    req.extensions_mut().insert(token_data.claims);
    req.extensions_mut()
        .insert(VerifiedModelBearer::new(&token));
    if let Some(data_plane_bearer) = data_plane_bearer {
        req.extensions_mut().insert(data_plane_bearer);
    }
    if let Some(capability_bearer) = capability_bearer {
        req.extensions_mut().insert(capability_bearer);
    }
    if let Some(cost_bearer) = cost_bearer {
        req.extensions_mut().insert(cost_bearer);
    }
    if let Some(session_bearer) = session_bearer {
        req.extensions_mut().insert(session_bearer);
    }
    if let Some(inference_bearer) = inference_bearer {
        req.extensions_mut().insert(inference_bearer);
    }
    if let Some(execution_bearer) = execution_bearer {
        req.extensions_mut().insert(execution_bearer);
    }
    if let Some(browser_bearer) = browser_bearer {
        req.extensions_mut().insert(browser_bearer);
    }
    if let Some(ingestion_bearer) = ingestion_bearer {
        req.extensions_mut().insert(ingestion_bearer);
    }
    Ok(next.run(req).await)
}

const SERVICE_SCOPE_MODELS_INVOKE: &str = "models:invoke";
const SERVICE_SCOPE_MODEL_SPACE_DELETE: &str = "model:space-delete";
const CONTROL_SPACE_DELETION_SERVICE: &str = "service:control-space-deletion";

/// Restrict service principals to the two non-persisting unary machine routes.
/// Users retain the existing route policy. This must run after [`require_auth`].
///
/// # Errors
/// Returns `401` when verified claims are absent/ambiguous and `403` when a
/// service principal lacks the exact route or `models:invoke` scope.
pub async fn authorize_principal_route(req: Request, next: Next) -> Result<Response, StatusCode> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    match claims.principal_kind()? {
        PrincipalKind::User => Ok(next.run(req).await),
        PrincipalKind::Service => {
            let allowed = match (req.method(), req.uri().path()) {
                (&axum::http::Method::POST, "/v1/ai/chat" | "/v1/ai/embeddings") => {
                    claims.has_scope(SERVICE_SCOPE_MODELS_INVOKE)
                }
                (&axum::http::Method::POST, "/v1/internal/space-deletion/threads") => {
                    claims.sub == CONTROL_SPACE_DELETION_SERVICE
                        && claims.has_scope(SERVICE_SCOPE_MODEL_SPACE_DELETE)
                }
                _ => false,
            };
            if !allowed {
                warn!(
                    service = %claims.sub,
                    method = %req.method(),
                    path = %req.uri().path(),
                    "service principal route or scope denied"
                );
                return Err(StatusCode::FORBIDDEN);
            }
            Ok(next.run(req).await)
        }
    }
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
        std::env::remove_var("DATA_PLANE_AUTH_AUDIENCE");
        std::env::remove_var("CAPABILITY_CORE_AUTH_AUDIENCE");
        std::env::remove_var("COST_CORE_AUTH_AUDIENCE");
        std::env::remove_var("SESSION_CORE_AUTH_AUDIENCE");
        std::env::remove_var("INFERENCE_CORE_AUTH_AUDIENCE");
        std::env::remove_var("EXECUTION_CORE_AUTH_AUDIENCE");
        std::env::remove_var("BROWSER_BROKER_AUTH_AUDIENCE");
        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
        std::env::remove_var("ALLOW_INSECURE_DEV_DEFAULTS");
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

    #[tokio::test]
    #[serial]
    async fn startup_jwks_warmup_rejects_redirects_and_oversized_documents() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        clear_env();
        reset_jwks_cache_for_test().await;
        let redirect_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "https://example.test/jwks"),
            )
            .mount(&redirect_server)
            .await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/jwks", redirect_server.uri()),
        );
        assert!(warm_jwks().await.is_err());

        reset_jwks_cache_for_test().await;
        let oversized_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_JWKS_BYTES + 1]))
            .mount(&oversized_server)
            .await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/jwks", oversized_server.uri()),
        );
        assert!(warm_jwks().await.is_err());
        clear_env();
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
            zdr: false,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
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
            zdr: false,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: Claims = serde_json::from_str(&s).unwrap();
        assert_eq!(back.aud.as_deref(), Some("model-gateway"));
        assert!(back.has_scope("models:invoke"));
        assert!(!back.has_scope("models:admin"));
    }

    #[test]
    fn startup_auth_posture_requires_complete_verification_and_two_dev_gates() {
        assert!(
            validate_startup_values(Some("jwks"), Some("issuer"), Some("aud"), false, false)
                .is_ok()
        );
        assert!(validate_startup_values(None, Some("issuer"), Some("aud"), false, false).is_err());
        assert!(validate_startup_values(Some("jwks"), None, Some("aud"), false, false).is_err());
        assert!(validate_startup_values(Some("jwks"), Some("issuer"), None, false, false).is_err());
        assert!(
            validate_startup_values(Some("jwks"), Some("issuer"), Some("aud"), true, false)
                .is_err()
        );
        assert!(validate_startup_values(None, None, None, true, true).is_ok());
    }

    #[test]
    fn require_org_matches_and_rejects() {
        let c = Claims {
            sub: "user-test".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "org-a".into(),
            user_id: "u".into(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: false,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
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
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");

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

    /// The bypass through the REAL layer stack, not `require_auth` alone.
    ///
    /// `dev_bypass_accepts_any_bearer` above layers only `require_auth`, so it
    /// proved the branch works in isolation while the deployed stack —
    /// `require_auth` wrapping `authorize_principal_route` wrapping
    /// `rate_limit_middleware` (`http_routes.rs`) — returned a bare 401 on every
    /// `/v1/*` route with the bypass on. Observed 2026-08-26 against the live
    /// container: the acceptance WARN was logged and the request was still
    /// refused before any handler ran. Isolation-only coverage is what let that
    /// stand.
    #[tokio::test]
    #[serial]
    async fn dev_bypass_survives_the_real_layer_stack() {
        async fn echo_claims(axum::Extension(claims): axum::Extension<Claims>) -> String {
            format!("{}:{}", claims.org_id, claims.user_id)
        }

        clear_env();
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");

        // Same order as `http_routes.rs`: the LAST `.layer` is outermost, so
        // require_auth runs first and authorize_principal_route sees its Claims.
        let app = Router::new()
            .route("/v1/models", get(echo_claims))
            .layer(middleware::from_fn(authorize_principal_route))
            .layer(middleware::from_fn(require_auth));

        let req = HttpRequest::builder()
            .uri("/v1/models")
            .header("authorization", "Bearer whatever")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "the bypass must produce a usable request through the layer stack, not just \
             through require_auth in isolation"
        );

        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    }

    /// A delegated header the caller DID send must still be verified, not
    /// shadowed by the synthesised one — otherwise the fix above would turn a
    /// forged delegate into an accepted one on the realistic SPA/BFF path.
    #[tokio::test]
    #[serial]
    async fn a_supplied_delegated_bearer_is_still_verified_under_the_bypass() {
        async fn ok() -> &'static str {
            "ok"
        }

        clear_env();
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");

        let app = Router::new()
            .route("/v1/models", get(ok))
            .layer(middleware::from_fn(require_auth));

        let req = HttpRequest::builder()
            .uri("/v1/models")
            .header("authorization", "Bearer whatever")
            // Present but junk: it must be REJECTED rather than silently
            // replaced by the synthesised bearer.
            .header("x-inference-authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "a supplied delegated bearer must go through verification even under the bypass"
        );

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
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");

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
    async fn verified_model_claims_are_available_only_after_jwt_verification() {
        async fn echo_claims(
            axum::Extension(claims): axum::Extension<Claims>,
            _bearer: VerifiedModelBearer,
        ) -> String {
            format!("{}:true", claims.user_id)
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-forwarding";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        let token = sign_jwt(&base_claims(), kid);

        let app = Router::new()
            .route("/", get(echo_claims))
            .layer(middleware::from_fn(require_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], b"user-test:true");
        clear_env();
    }

    #[test]
    fn verified_model_bearer_debug_output_never_exposes_the_token() {
        let bearer = VerifiedModelBearer::for_test("sensitive-token-value");
        let debug = format!("{bearer:?}");
        assert_eq!(debug, "VerifiedModelBearer([REDACTED])");
        assert!(!debug.contains("sensitive-token-value"));
    }

    #[test]
    fn verified_session_bearer_debug_output_never_exposes_the_token() {
        let bearer = VerifiedSessionBearer::for_test("sensitive-session-token-value");
        let debug = format!("{bearer:?}");
        assert_eq!(debug, "VerifiedSessionBearer([REDACTED])");
        assert!(!debug.contains(bearer.as_str()));
    }

    #[test]
    fn verified_inference_bearer_debug_output_never_exposes_the_token() {
        let bearer = VerifiedInferenceBearer::for_test("sensitive-inference-token-value");
        let debug = format!("{bearer:?}");
        assert_eq!(debug, "VerifiedInferenceBearer([REDACTED])");
        assert!(!debug.contains(bearer.as_str()));
    }

    #[test]
    fn verified_browser_bearer_debug_output_never_exposes_the_token() {
        let bearer = VerifiedBrowserBearer::for_test("sensitive-browser-token-value");
        let debug = format!("{bearer:?}");
        assert_eq!(debug, "VerifiedBrowserBearer([REDACTED])");
        assert!(!debug.contains(bearer.as_str()));
    }

    #[tokio::test]
    #[serial]
    async fn delegated_inference_bearer_requires_exact_audience_and_matching_identity() {
        async fn echo_inference_bearer(bearer: VerifiedInferenceBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-inference-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut inference_claims = base_claims();
        inference_claims.aud = Some("inference-core".into());
        let inference_token = sign_jwt(&inference_claims, kid);

        let response = Router::new()
            .route("/", get(echo_inference_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-inference-authorization",
                        format!("Bearer {inference_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], inference_token.as_bytes());

        let mut wrong_audience = base_claims();
        wrong_audience.aud = Some("model-gateway".into());
        let wrong_audience = sign_jwt(&wrong_audience, kid);
        let response = Router::new()
            .route("/", get(echo_inference_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-inference-authorization",
                        format!("Bearer {wrong_audience}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut wrong_identity = inference_claims;
        wrong_identity.sub = "user-other".into();
        wrong_identity.user_id = "user-other".into();
        let wrong_identity = sign_jwt(&wrong_identity, kid);
        let response = Router::new()
            .route("/", get(echo_inference_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-inference-authorization",
                        format!("Bearer {wrong_identity}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_browser_bearer_requires_exact_audience_and_matching_identity() {
        async fn echo_browser_bearer(bearer: VerifiedBrowserBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-browser-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("BROWSER_BROKER_AUTH_AUDIENCE", "browser-broker");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut browser_claims = base_claims();
        browser_claims.aud = Some("browser-broker".into());
        let browser_token = sign_jwt(&browser_claims, kid);

        let response = Router::new()
            .route("/", get(echo_browser_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-browser-authorization", format!("Bearer {browser_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let wrong_audience = sign_jwt(&base_claims(), kid);
        let response = Router::new()
            .route("/", get(echo_browser_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-browser-authorization",
                        format!("Bearer {wrong_audience}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut other_tenant = browser_claims;
        other_tenant.org_id = "org-other".into();
        let other_tenant = sign_jwt(&other_tenant, kid);
        let response = Router::new()
            .route("/", get(echo_browser_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-browser-authorization", format!("Bearer {other_tenant}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_execution_bearer_requires_exact_audience_and_matching_identity() {
        async fn echo_execution_bearer(bearer: VerifiedExecutionBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-execution-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("EXECUTION_CORE_AUTH_AUDIENCE", "execution-core");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut execution_claims = base_claims();
        execution_claims.aud = Some("execution-core".into());
        let execution_token = sign_jwt(&execution_claims, kid);

        let response = Router::new()
            .route("/", get(echo_execution_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-execution-authorization",
                        format!("Bearer {execution_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut data_claims = base_claims();
        data_claims.aud = Some("data-plane".into());
        let data_token = sign_jwt(&data_claims, kid);
        let response = Router::new()
            .route("/", get(echo_execution_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-execution-authorization", format!("Bearer {data_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        execution_claims.sub = "user-other".into();
        execution_claims.user_id = "user-other".into();
        let wrong_identity = sign_jwt(&execution_claims, kid);
        let response = Router::new()
            .route("/", get(echo_execution_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-execution-authorization",
                        format!("Bearer {wrong_identity}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_session_bearer_requires_exact_audience_and_matching_identity() {
        async fn echo_session_bearer(bearer: VerifiedSessionBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-session-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut session_claims = base_claims();
        session_claims.aud = Some("session-core".into());
        let session_token = sign_jwt(&session_claims, kid);

        let response = Router::new()
            .route("/", get(echo_session_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-session-authorization", format!("Bearer {session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], session_token.as_bytes());

        let mut data_plane_claims = base_claims();
        data_plane_claims.aud = Some("data-plane".into());
        let data_plane_token = sign_jwt(&data_plane_claims, kid);
        let response = Router::new()
            .route("/", get(echo_session_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-session-authorization",
                        format!("Bearer {data_plane_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut other_tenant_claims = session_claims;
        other_tenant_claims.org_id = "org-other".into();
        let other_tenant_token = sign_jwt(&other_tenant_claims, kid);
        let response = Router::new()
            .route("/", get(echo_session_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-session-authorization",
                        format!("Bearer {other_tenant_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_data_plane_bearer_requires_data_audience_and_matching_identity() {
        async fn echo_data_bearer(bearer: VerifiedDataPlaneBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-data-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut data_claims = base_claims();
        data_claims.aud = Some("data-plane".into());
        let data_token = sign_jwt(&data_claims, kid);

        let app = Router::new()
            .route("/", get(echo_data_bearer))
            .layer(middleware::from_fn(require_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-data-plane-authorization", format!("Bearer {data_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], data_token.as_bytes());

        let wrong_audience = sign_jwt(&base_claims(), kid);
        let response = Router::new()
            .route("/", get(echo_data_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-data-plane-authorization",
                        format!("Bearer {wrong_audience}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_capability_bearer_requires_its_own_audience_and_identity() {
        async fn echo_capability_bearer(bearer: VerifiedCapabilityBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-capability-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut capability_claims = base_claims();
        capability_claims.aud = Some("capability-core".into());
        let capability_token = sign_jwt(&capability_claims, kid);

        let app = Router::new()
            .route("/", get(echo_capability_bearer))
            .layer(middleware::from_fn(require_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-capability-authorization",
                        format!("Bearer {capability_token}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(&body[..], capability_token.as_bytes());

        let mut wrong_identity = base_claims();
        wrong_identity.aud = Some("capability-core".into());
        wrong_identity.sub = "user-other".into();
        wrong_identity.user_id = "user-other".into();
        let wrong_identity = sign_jwt(&wrong_identity, kid);
        let response = Router::new()
            .route("/", get(echo_capability_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-capability-authorization",
                        format!("Bearer {wrong_identity}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn dev_bypass_cannot_supply_a_data_plane_bearer() {
        async fn requires_verified_bearer(_bearer: VerifiedDataPlaneBearer) -> &'static str {
            "unreachable"
        }

        clear_env();
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
        let app = Router::new()
            .route("/", get(requires_verified_bearer))
            .layer(middleware::from_fn(require_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", "Bearer unverified-dev-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        clear_env();
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

    fn sign_test_jwt(claims: &impl Serialize, kid: &str) -> String {
        use jsonwebtoken::{encode, EncodingKey, Header};
        let (priv_pem, _) = rsa_keypair_pem();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let key = EncodingKey::from_rsa_pem(priv_pem.as_bytes()).expect("load priv key");
        encode(&header, claims, &key).expect("encode jwt")
    }

    fn sign_jwt(claims: &Claims, kid: &str) -> String {
        sign_test_jwt(claims, kid)
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
            sub: "user-test".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-test".into(),
            user_id: "user-test".into(),
            nbf: None,
            aud: Some("model-gateway".into()),
            scopes: Vec::new(),
            zdr: false,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn service_claims() -> serde_json::Value {
        serde_json::json!({
            "sub": "service:model-worker",
            "iss": "auth-core",
            "aud": "model-gateway",
            "exp": now_secs() + 300,
            "nbf": now_secs() - 5,
            "org_id": "org-test",
            "principal_type": "service",
            "service_id": "service:model-worker",
            "reason": "invoke bounded model primitive",
            "scopes": ["models:invoke"],
            "zdr": true
        })
    }

    #[tokio::test]
    #[serial]
    async fn canonical_signed_service_identity_is_accepted_without_user_impersonation() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-service-identity";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

        let claims = service_claims();
        assert!(claims.get("user_id").is_none());
        let token = sign_test_jwt(&claims, kid);
        assert_eq!(send_with_token(&token).await, StatusCode::OK);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn signed_service_identity_rejects_missing_wrong_or_ambiguous_claims() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-service-invalid";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

        let mut invalid = Vec::new();
        let mut missing_service_id = service_claims();
        missing_service_id
            .as_object_mut()
            .unwrap()
            .remove("service_id");
        invalid.push(missing_service_id);
        let mut wrong_service_id = service_claims();
        wrong_service_id["service_id"] = serde_json::json!("service:other");
        invalid.push(wrong_service_id);
        let mut impersonates_user = service_claims();
        impersonates_user["user_id"] = serde_json::json!("user-victim");
        invalid.push(impersonates_user);
        let mut ambiguous_type = service_claims();
        ambiguous_type["principal_type"] = serde_json::json!("robot");
        invalid.push(ambiguous_type);
        let mut missing_reason = service_claims();
        missing_reason.as_object_mut().unwrap().remove("reason");
        invalid.push(missing_reason);
        let mut blank_reason = service_claims();
        blank_reason["reason"] = serde_json::json!("   ");
        invalid.push(blank_reason);
        let mut oversized_reason = service_claims();
        oversized_reason["reason"] = serde_json::json!("x".repeat(501));
        invalid.push(oversized_reason);

        for claims in invalid {
            let token = sign_test_jwt(&claims, kid);
            assert_eq!(send_with_token(&token).await, StatusCode::UNAUTHORIZED);
        }
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn signed_service_identity_enforces_audience_issuer_org_and_time() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-service-standard-claims";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

        let mut invalid = Vec::new();
        let mut wrong_audience = service_claims();
        wrong_audience["aud"] = serde_json::json!("data-plane");
        invalid.push(wrong_audience);
        let mut wrong_issuer = service_claims();
        wrong_issuer["iss"] = serde_json::json!("self-forged");
        invalid.push(wrong_issuer);
        let mut empty_org = service_claims();
        empty_org["org_id"] = serde_json::json!("");
        invalid.push(empty_org);
        let mut expired = service_claims();
        expired["exp"] = serde_json::json!(now_secs() - 120);
        invalid.push(expired);
        let mut not_yet_valid = service_claims();
        not_yet_valid["nbf"] = serde_json::json!(now_secs() + 120);
        invalid.push(not_yet_valid);

        for claims in invalid {
            let token = sign_test_jwt(&claims, kid);
            assert_eq!(send_with_token(&token).await, StatusCode::UNAUTHORIZED);
        }
        clear_env();
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
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");

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
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
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
    async fn missing_audience_env_always_fails_closed() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-aud";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        // AUTH_CORE_AUDIENCE intentionally unset.

        let mut claims = base_claims();
        // Token still carries an aud; middleware must refuse to validate.
        claims.aud = Some("model-gateway".into());
        let token = sign_jwt(&claims, kid);

        assert_eq!(
            send_with_token(&token).await,
            StatusCode::INTERNAL_SERVER_ERROR,
        );

        // The removed legacy opt-out must not weaken validation.
        std::env::set_var("AUTH_CORE_AUDIENCE_OPTIONAL", "1");
        assert_eq!(
            send_with_token(&token).await,
            StatusCode::INTERNAL_SERVER_ERROR,
        );

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

    #[tokio::test]
    async fn only_the_exact_deletion_coordinator_reaches_the_internal_space_delete_route() {
        async fn ok_handler() -> &'static str {
            "ok"
        }

        fn service(service_id: &str, scopes: &[&str]) -> Claims {
            Claims {
                sub: service_id.to_owned(),
                iss: "auth-core".to_owned(),
                exp: now_secs() + 60,
                org_id: "org-test".to_owned(),
                user_id: String::new(),
                nbf: None,
                aud: Some("model-gateway".to_owned()),
                scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                zdr: false,
                principal_type: Some("service".to_owned()),
                service_id: Some(service_id.to_owned()),
                reason: Some("execute Control-authorized Space deletion".to_owned()),
            }
        }

        fn app(claims: Claims) -> Router {
            Router::new()
                .route(
                    "/v1/internal/space-deletion/threads",
                    axum::routing::post(ok_handler),
                )
                .layer(middleware::from_fn(authorize_principal_route))
                .layer(axum::Extension(claims))
        }

        let allowed = app(service(
            CONTROL_SPACE_DELETION_SERVICE,
            &[SERVICE_SCOPE_MODEL_SPACE_DELETE],
        ))
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/v1/internal/space-deletion/threads")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);

        for claims in [
            service(
                "service:control-space-deletion-copy",
                &[SERVICE_SCOPE_MODEL_SPACE_DELETE],
            ),
            service(CONTROL_SPACE_DELETION_SERVICE, &[]),
            service(
                CONTROL_SPACE_DELETION_SERVICE,
                &[SERVICE_SCOPE_MODELS_INVOKE],
            ),
        ] {
            let response = app(claims)
                .oneshot(
                    HttpRequest::builder()
                        .method("POST")
                        .uri("/v1/internal/space-deletion/threads")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }
}
