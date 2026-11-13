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
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{
    decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn};

/// The `zdr_waiver` claim: one user connection runs with zero retention off for one turn, because
/// the user acknowledged that it cannot give it (Z-14). It binds exactly one provider and one
/// connection, and a version the gateway knows. The resolver refuses it anywhere else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZdrWaiverClaim {
    /// The waiver contract version the issuer minted it under.
    pub version: u32,
    /// The provider the waiver binds, spelled exactly as minted.
    pub provider: String,
    /// The one user connection the waiver binds.
    pub connection: String,
}

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
    /// Required, not defaulted: Auth Core stamps it on every plane token, so an
    /// absent claim means a broken mint and is refused rather than read as
    /// persistent.
    pub zdr: bool,
    /// The org's standing zero-retention posture, as the issuer read it when it minted the token:
    /// `Some(true)` for an org that is ZDR. Absent (`None`) on a service token and on a token minted
    /// before the claim existed. It is an input to the resolver only, never a decision on its own
    /// (see `zdr_posture`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_zdr: Option<bool>,
    /// The connection-bound waiver of zero retention, when the issuer minted one for this turn (see
    /// [`ZdrWaiverClaim`]). Absent on every token that does not waive ZDR for a connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zdr_waiver: Option<ZdrWaiverClaim>,
    /// Sovereign-infrastructure posture asserted by the verified issuer: this
    /// principal's content may only be processed on sovereign (currently
    /// Norwegian) infrastructure. A distinct axis from [`Self::zdr`] — ZDR is
    /// about RETENTION, sovereignty is about JURISDICTION — and Data Plane v2
    /// reads it under the same name from the same auth-core token.
    ///
    /// Tri-state on purpose, and NOT `bool` like `zdr` above. `None` means the
    /// claim is genuinely ABSENT, which is today's reality for every real
    /// token: auth-core's `issuePlaneToken` has never minted this claim. That
    /// is different from a signed `Some(false)`, and collapsing the two is
    /// what took Data Plane v2's dense arm down — see
    /// [`mp_contracts::dataplane_posture`] for the full ladder and for what
    /// silence resolves to.
    #[serde(default)]
    pub sovereign: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// INNH-10: the API key an `api_key` principal was minted for (the Better Auth key id, never
    /// the key). Present only on an `api_key` token; on any other principal type it makes the
    /// token ambiguous and [`Self::principal_kind`] refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// INNH-10: the user who owns the API key of an `api_key` principal. Attribution only: the
    /// principal is the key (`sub = apikey:<key_id>`), never this person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_user_id: Option<String>,
}

/// The `step_up` claim: the holder re-proved their password at `at` (Unix seconds), so a
/// high-risk approval may be granted within its short validity (build plan WS13 S5). Auth Core
/// mints it only after a fresh check, and only while that check is recent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepUpClaim {
    pub at: i64,
    pub method: String,
}

/// The payload of a verified model token: the identity claims and the optional step-up proof.
/// Both come from the one signature check, so no path reads a proof from an unverified token.
#[derive(Debug, Deserialize)]
struct ModelTokenPayload {
    #[serde(flatten)]
    identity: Claims,
    #[serde(default)]
    step_up: Option<StepUpClaim>,
}

/// The step-up proof the verified model token carried, if any. `require_auth` inserts it beside
/// the [`Claims`]; the development bypass verifies nothing and so inserts none.
#[derive(Debug, Clone, Default)]
pub struct VerifiedStepUp(pub Option<StepUpClaim>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalKind {
    User,
    Service,
    /// INNH-10: an API key exchanged by Auth Core for a short-lived token. Admitted ONLY on the
    /// key-grant routes ([`API_KEY_ROUTES`]) and, while its switch is on, the Agent API routes
    /// ([`AGENT_API_KEY_ROUTES`], EP-12 S5); refused on every other route by
    /// [`authorize_principal_route`] and on the gRPC surface by `grpc_auth`.
    ApiKey,
}

/// The subject prefix of an `api_key` principal: `sub` is exactly `apikey:<key_id>`.
const API_KEY_SUBJECT_PREFIX: &str = "apikey:";
/// The scope an `api_key` principal needs on the key-grant routes (INNH-10 OD-5).
pub(crate) const API_KEY_SCOPE_BROWSER_AGENT: &str = "browser:agent";
const MAX_API_KEY_ID_LEN: usize = 128;
const MAX_ACTING_USER_ID_LEN: usize = 256;

/// The only routes an `api_key` principal may reach, as `(method, axum path)`. Everything else
/// is refused with 403 by [`authorize_principal_route`].
pub const API_KEY_ROUTES: &[(&str, &str)] = &[
    ("POST", "/v1/browser/key-grants"),
    ("DELETE", "/v1/browser/key-grants/:grant_id"),
];

/// Whether `method path` (a concrete request path) is one of [`API_KEY_ROUTES`]. A grant id is
/// one non-empty path segment; the handler validates its characters.
fn api_key_route_allowed(method: &str, path: &str) -> bool {
    match method {
        "POST" => path == "/v1/browser/key-grants",
        "DELETE" => path
            .strip_prefix("/v1/browser/key-grants/")
            .is_some_and(|grant_id| !grant_id.is_empty() && !grant_id.contains('/')),
        _ => false,
    }
}

/// EP-12 S5: the scope an `api_key` principal needs on the Agent API routes (D3, D4). The
/// key-grant routes keep `browser:agent`; neither scope opens the other's routes.
pub(crate) const API_KEY_SCOPE_AGENT_RUNS: &str = "agent:runs";

/// EP-12 S5: the Agent API routes an `api_key` principal reaches while `MODEL_GATEWAY_AGENT_API`
/// is on, as `(method, axum path)`. Off, [`API_KEY_ROUTES`] is the whole allow-list.
pub const AGENT_API_KEY_ROUTES: &[(&str, &str)] = &[
    // EP-12 S7: start an Ask run.
    ("POST", "/v1/agent/runs"),
    ("GET", "/v1/agent/runs/:run_id"),
    ("GET", "/v1/agent/runs/:run_id/events"),
    ("POST", "/v1/agent/runs/:run_id/cancel"),
    ("GET", "/v1/agent/threads/:thread_id/runs"),
    ("GET", "/v1/agent/threads/:thread_id/messages"),
];

/// The one downstream header a key may send, and only on an Agent API route: its own
/// `aud=session-core` bearer.
const API_KEY_SESSION_HEADER: &str = "x-session-authorization";

/// EP-12 S7: the second downstream header a key may send, and only on the start route: its own
/// `aud=inference-core` bearer, which runs the Ask turn.
const API_KEY_INFERENCE_HEADER: &str = "x-inference-authorization";

/// EP-12 S7: whether `method path` is the Agent API start route, the only route that calls a
/// model with the key's inference bearer.
fn agent_api_start_route(method: &str, path: &str) -> bool {
    method == "POST" && path == "/v1/agent/runs"
}

/// Whether `method path` (a concrete request path) is one of [`AGENT_API_KEY_ROUTES`]. An id is
/// one non-empty path segment; the handler validates its characters.
fn agent_api_route(method: &str, path: &str) -> bool {
    AGENT_API_KEY_ROUTES
        .iter()
        .any(|(route_method, pattern)| *route_method == method && path_matches(pattern, path))
}

/// Whether `path` fills `pattern` segment for segment, a `:name` segment taking any one
/// non-empty segment. A trailing slash is one more (empty) segment, so it never matches.
fn path_matches(pattern: &str, path: &str) -> bool {
    let mut patterns = pattern.split('/');
    let mut segments = path.split('/');
    loop {
        match (patterns.next(), segments.next()) {
            (None, None) => return true,
            (Some(expected), Some(segment)) => {
                let fits = if expected.starts_with(':') {
                    !segment.is_empty()
                } else {
                    expected == segment
                };
                if !fits {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// The scope a key needs for `method path`, or `None` when no key may reach it. The Agent API
/// routes exist for a key only while `agent_api` (the switch) is on.
fn api_key_route_scope(method: &str, path: &str, agent_api: bool) -> Option<&'static str> {
    if api_key_route_allowed(method, path) {
        Some(API_KEY_SCOPE_BROWSER_AGENT)
    } else if agent_api && agent_api_route(method, path) {
        Some(API_KEY_SCOPE_AGENT_RUNS)
    } else {
        None
    }
}

/// A key id as the broker accepts it: 1 to 128 bytes of printable, non-space ASCII.
fn valid_api_key_id(key_id: &str) -> bool {
    (1..=MAX_API_KEY_ID_LEN).contains(&key_id.len())
        && key_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
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

/// Independently verified `aud=quarry` user bearer of the caller, forwarded by the BFF as
/// `x-quarry-authorization` (Teach mode, F-t3-081). Model Gateway uses it only to read one run's
/// procedure draft as the person who drove the run. It is never the gateway's own service
/// principal, which is not that run's actor, and it is never accepted from a body or a query.
#[derive(Clone)]
pub struct VerifiedQuarryBearer(Arc<str>);

impl VerifiedQuarryBearer {
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

/// INNH-10: the independently verified `aud=browser-broker` bearer of an `api_key` principal.
/// Accepted only from `x-browser-authorization`, only on the key-grant routes, and only when it
/// is the same key as the ingress token (same `sub`, `org_id`, `key_id`, `acting_user_id` and
/// retention posture). A distinct type from [`VerifiedBrowserBearer`] on purpose: no user route
/// can ever extract a key's bearer, and the key-grant routes can never extract a user's.
#[derive(Clone)]
pub struct VerifiedApiKeyBrowserBearer(Arc<str>);

impl VerifiedApiKeyBrowserBearer {
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

impl fmt::Debug for VerifiedApiKeyBrowserBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedApiKeyBrowserBearer([REDACTED])")
    }
}

/// EP-12 S5: the independently verified `aud=session-core` bearer of an `api_key` principal.
/// Accepted only from `x-session-authorization`, only on an Agent API route while the switch is
/// on, exactly once, and only as the same key as the ingress token (same `sub`, `org_id`,
/// `key_id`, `acting_user_id` and retention posture) holding `agent:runs`. A distinct type from
/// [`VerifiedSessionBearer`] on purpose: no user route can extract a key's session bearer.
#[derive(Clone)]
pub struct VerifiedApiKeySessionBearer(Arc<str>);

impl VerifiedApiKeySessionBearer {
    fn new(token: &str) -> Self {
        Self(Arc::from(token))
    }

    /// The key's bearer as the session credential the shared Session Core request helpers and
    /// the run-events stream forward. Only an Agent API handler holds a key bearer, and Session
    /// Core binds every call it carries to the key's own threads and runs.
    pub(crate) fn as_session_bearer(&self) -> VerifiedSessionBearer {
        VerifiedSessionBearer::new(&self.0)
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self::new(token)
    }
}

impl fmt::Debug for VerifiedApiKeySessionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedApiKeySessionBearer([REDACTED])")
    }
}

/// EP-12 S7: the independently verified `aud=inference-core` bearer of an `api_key` principal.
/// Accepted only from `x-inference-authorization`, only on the start route while the switch is
/// on, exactly once, and only as the same key as the ingress token holding `agent:runs`, with the
/// same retention posture. A distinct type from [`VerifiedInferenceBearer`] on purpose: no user
/// route can extract a key's inference bearer, and the start route can never extract a user's.
#[derive(Clone)]
pub struct VerifiedApiKeyInferenceBearer(Arc<str>);

impl VerifiedApiKeyInferenceBearer {
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

impl fmt::Debug for VerifiedApiKeyInferenceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedApiKeyInferenceBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedApiKeyBrowserBearer
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

/// Independently verified `aud=sandbox-manager` user bearer. Accepted only
/// from `x-sandbox-authorization`, bound to the ingress user and tenant, and
/// present only on a Space-scoped turn (the V3 BFF mints it after Control has
/// scoped the thread). Forwarded unchanged to execution-core, which presents
/// it to sandbox-manager's `AcquireLease` — the one lease RPC whose identity
/// check binds the Space capability decision to the CALLER's verified subject,
/// so only a user-bound credential can pass it. See
/// apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
/// §3.5 phase B.2.
#[derive(Clone)]
pub struct VerifiedSandboxBearer(Arc<str>);

impl VerifiedSandboxBearer {
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

impl fmt::Debug for VerifiedSandboxBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedSandboxBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedSandboxBearer
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

impl fmt::Debug for VerifiedQuarryBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerifiedQuarryBearer([REDACTED])")
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for VerifiedQuarryBearer
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
                    && self.service_id.is_none()
                    && self.key_id.is_none() =>
            {
                Ok(PrincipalKind::User)
            }
            "api_key" if self.is_well_formed_api_key() => Ok(PrincipalKind::ApiKey),
            "service"
                if self.user_id.is_empty()
                    && self.key_id.is_none()
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

    /// The shape rules of an `api_key` token (INNH-10, the same rules the broker's `authctx`
    /// applies): `sub == "apikey:" + key_id` exactly, a bounded printable key id, an owning
    /// `acting_user_id` that is unpadded and not itself a key, and no user or service identity.
    fn is_well_formed_api_key(&self) -> bool {
        let (Some(key_id), Some(owner)) = (self.key_id.as_deref(), self.acting_user_id.as_deref())
        else {
            return false;
        };
        valid_api_key_id(key_id)
            && self.sub.strip_prefix(API_KEY_SUBJECT_PREFIX) == Some(key_id)
            && self.user_id.is_empty()
            && self.service_id.is_none()
            && !owner.is_empty()
            && owner.len() <= MAX_ACTING_USER_ID_LEN
            && owner == owner.trim()
            && !owner.starts_with(API_KEY_SUBJECT_PREFIX)
    }

    /// The key id of a well-formed `api_key` principal, `None` for every other principal.
    #[must_use]
    pub fn api_key_id(&self) -> Option<&str> {
        if self.principal_kind() == Ok(PrincipalKind::ApiKey) {
            self.key_id.as_deref()
        } else {
            None
        }
    }

    /// The service id of a SERVICE principal, `None` for a user. Reads the claim only; whether the
    /// claim set is well formed is [`Self::principal_kind`]'s question, asked by the route policy.
    /// The rate limiter keys a service's bucket on this.
    #[must_use]
    pub fn service_principal_id(&self) -> Option<&str> {
        if self.principal_type.as_deref() == Some("service") {
            self.service_id.as_deref().filter(|id| !id.is_empty())
        } else {
            None
        }
    }

    /// Returns `true` if the token carries the given scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// The sovereignty posture to put on a Data Plane v2 `RetrieveRequest`,
    /// combining this token's signed claim with whatever the caller declared
    /// for one request. The retention axis is `zdr_posture::decide_for`; this is
    /// its sibling for the other privacy axis. The merge rules live in
    /// [`mp_contracts::dataplane_posture::effective_sovereign_required`].
    #[must_use]
    pub const fn effective_sovereign_required(&self, requested: Option<bool>) -> bool {
        mp_contracts::dataplane_posture::effective_sovereign_required(self.sovereign, requested)
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

/// The bypass asserts no retention posture of its own. It stands for the
/// persistent (non-ZDR) posture, so a ZDR delegate for the same identity is
/// refused under it instead of being served as persistent.
fn dev_bypass_claims(headers: &HeaderMap) -> Claims {
    let user_id = header_string(headers, "x-user-id", "user_placeholder");
    let org_id = header_string(headers, "x-org-id", "org_placeholder");
    Claims {
        key_id: None,
        acting_user_id: None,
        sub: user_id.clone(),
        iss: "dev".to_owned(),
        exp: i64::MAX,
        org_id,
        user_id,
        nbf: None,
        aud: None,
        scopes: Vec::new(),
        zdr: false,
        sovereign: None,
        org_zdr: None,
        zdr_waiver: None,
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
) -> Result<Option<VerifiedDataPlaneBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedIngestionBearer>, AuthRejection> {
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

/// The caller's `aud=quarry` bearer (Teach mode, F-t3-081), verified like every user bearer: the
/// same subject, user and tenant as the ingress token, and the same retention posture. Its audience
/// is `QUARRY_AUTH_AUDIENCE`, which defaults to `quarry` as the edge's own audience does.
async fn verify_delegated_quarry_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedQuarryBearer>, AuthRejection> {
    verify_delegated_user_bearer(
        headers,
        "x-quarry-authorization",
        "QUARRY_AUTH_AUDIENCE",
        "quarry",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedQuarryBearer::new(&token)))
}

async fn verify_delegated_capability_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedCapabilityBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedCostBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedSessionBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedInferenceBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedExecutionBearer>, AuthRejection> {
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
) -> Result<Option<VerifiedBrowserBearer>, AuthRejection> {
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

async fn verify_delegated_sandbox_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedSandboxBearer>, AuthRejection> {
    verify_delegated_user_bearer(
        headers,
        "x-sandbox-authorization",
        "SANDBOX_MANAGER_AUTH_AUDIENCE",
        "sandbox-manager",
        model_claims,
    )
    .await
    .map(|token| token.map(|token| VerifiedSandboxBearer::new(&token)))
}

async fn verify_delegated_user_bearer(
    headers: &HeaderMap,
    header_name: &'static str,
    audience_env: &'static str,
    default_audience: &'static str,
    model_claims: &Claims,
) -> Result<Option<String>, AuthRejection> {
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
        return Err(StatusCode::FORBIDDEN.into());
    }
    // Every user bearer of one turn carries the ingress token's retention
    // posture, waiver included. Auth Core mints them from one org decision, so a
    // bearer that says otherwise is a mixed set: a plane that reads only its own
    // claim would treat ZDR content as persistent. Refused fail closed, with the
    // typed body. The resolver (`zdr_posture`) makes the same check again.
    if claims.zdr != model_claims.zdr || claims.zdr_waiver != model_claims.zdr_waiver {
        warn!(
            audience = default_audience,
            "delegated user bearer retention posture differs from the ingress token"
        );
        return Err(AuthRejection::ZdrPostureMismatch);
    }
    Ok(Some(token))
}

/// The delegated headers an `api_key` principal may never send: every downstream bearer except
/// the broker one. A key has no user identity, so none of them can be bound to it.
const API_KEY_FORBIDDEN_DELEGATIONS: &[&str] = &[
    "x-data-plane-authorization",
    "x-capability-authorization",
    "x-cost-authorization",
    "x-session-authorization",
    "x-inference-authorization",
    "x-execution-authorization",
    "x-ingestion-authorization",
    "x-sandbox-authorization",
    "x-quarry-authorization",
];

/// INNH-10: verify the `aud=browser-broker` bearer of an `api_key` principal. It is accepted only
/// on the key-grant routes and only as the same key as the ingress token: same `sub`, `org_id`,
/// `key_id` and `acting_user_id`, and the same retention posture (a mixed set is the typed
/// `zdr_posture_mismatch`, as for a user's bearers). Anything else is a 403.
async fn verify_delegated_api_key_browser_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
    method: &str,
    path: &str,
) -> Result<Option<VerifiedApiKeyBrowserBearer>, AuthRejection> {
    if !headers.contains_key("x-browser-authorization") {
        return Ok(None);
    }
    if !api_key_route_allowed(method, path) {
        return Err(StatusCode::FORBIDDEN.into());
    }
    let Some((token, claims)) = decode_delegated_bearer(
        headers,
        "x-browser-authorization",
        "BROWSER_BROKER_AUTH_AUDIENCE",
        "browser-broker",
    )
    .await?
    else {
        return Ok(None);
    };
    if claims.principal_kind() != Ok(PrincipalKind::ApiKey)
        || claims.sub != model_claims.sub
        || claims.org_id != model_claims.org_id
        || claims.key_id != model_claims.key_id
        || claims.acting_user_id != model_claims.acting_user_id
    {
        return Err(StatusCode::FORBIDDEN.into());
    }
    if claims.zdr != model_claims.zdr || claims.zdr_waiver != model_claims.zdr_waiver {
        warn!("delegated api key bearer retention posture differs from the ingress token");
        return Err(AuthRejection::ZdrPostureMismatch);
    }
    Ok(Some(VerifiedApiKeyBrowserBearer::new(&token)))
}

/// EP-12 S5: verify the `aud=session-core` bearer of an `api_key` principal on an Agent API route
/// (the caller checks the route and the switch). Exactly one header: a repeated one is a 403, so
/// no proxy can pick a different value than the one verified. The bearer must be the same key as
/// the ingress token and hold `agent:runs`; a different retention posture is the typed
/// `zdr_posture_mismatch`. Anything else is a 403.
async fn verify_delegated_api_key_session_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedApiKeySessionBearer>, AuthRejection> {
    verify_delegated_api_key_agent_bearer(
        headers,
        model_claims,
        API_KEY_SESSION_HEADER,
        "SESSION_CORE_AUTH_AUDIENCE",
        "session-core",
    )
    .await
    .map(|token| token.map(|token| VerifiedApiKeySessionBearer::new(&token)))
}

/// EP-12 S7: verify the `aud=inference-core` bearer of an `api_key` principal on the start route
/// (the caller checks the route and the switch), under the same rules as its session bearer.
async fn verify_delegated_api_key_inference_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
) -> Result<Option<VerifiedApiKeyInferenceBearer>, AuthRejection> {
    verify_delegated_api_key_agent_bearer(
        headers,
        model_claims,
        API_KEY_INFERENCE_HEADER,
        "INFERENCE_CORE_AUTH_AUDIENCE",
        "inference-core",
    )
    .await
    .map(|token| token.map(|token| VerifiedApiKeyInferenceBearer::new(&token)))
}

/// The binding rules every Agent API downstream bearer of a key shares: exactly one header, the
/// named audience, the same key as the ingress token holding `agent:runs`, and the same retention
/// posture (a different one is the typed `zdr_posture_mismatch`). Anything else is a 403.
async fn verify_delegated_api_key_agent_bearer(
    headers: &HeaderMap,
    model_claims: &Claims,
    header_name: &'static str,
    audience_env: &'static str,
    default_audience: &'static str,
) -> Result<Option<String>, AuthRejection> {
    match headers.get_all(header_name).iter().count() {
        0 => return Ok(None),
        1 => {}
        _ => return Err(StatusCode::FORBIDDEN.into()),
    }
    let Some((token, claims)) =
        decode_delegated_bearer(headers, header_name, audience_env, default_audience).await?
    else {
        return Ok(None);
    };
    if claims.principal_kind() != Ok(PrincipalKind::ApiKey)
        || claims.sub != model_claims.sub
        || claims.org_id != model_claims.org_id
        || claims.key_id != model_claims.key_id
        || claims.acting_user_id != model_claims.acting_user_id
        || !claims.has_scope(API_KEY_SCOPE_AGENT_RUNS)
    {
        return Err(StatusCode::FORBIDDEN.into());
    }
    if claims.zdr != model_claims.zdr || claims.zdr_waiver != model_claims.zdr_waiver {
        warn!(
            audience = default_audience,
            "delegated api key bearer retention posture differs from the ingress token"
        );
        return Err(AuthRejection::ZdrPostureMismatch);
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
        // Downstream authority belongs to Inference Core's audience. The
        // separate gateway credential still requires `models:invoke` below.
        || !claims.has_scope(SERVICE_SCOPE_INFERENCE_INVOKE)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(VerifiedInferenceBearer::new(&token)))
}

/// The judge route's caller check (EP-7 S6), held to the rules of the delegated service bearers: the
/// verified token must be orchestrator-core's own service principal (its `service_id` is its `sub`),
/// hold the judge scope, and name the org the request body names. A token that is not a well-formed
/// principal is `401`; any other caller is `403`.
pub(crate) fn verify_goal_judge_caller(claims: &Claims, org_id: &str) -> Result<(), StatusCode> {
    if claims.principal_kind()? != PrincipalKind::Service
        || claims.sub != SERVICE_GOAL_JUDGE_CALLER
        || claims.service_id.as_deref() != Some(SERVICE_GOAL_JUDGE_CALLER)
        || !claims.has_scope(SERVICE_SCOPE_MODEL_GOAL_JUDGE)
        || claims.org_id.is_empty()
        || claims.org_id != org_id
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
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

/// Why a request's bearer set was refused. Every refusal is a bare status except
/// one: a delegated user bearer whose retention posture differs from the ingress
/// token. That refusal carries a typed body (`zdr_posture_mismatch`), so a caller
/// can tell it from an ordinary 403: a fresh mint can clear it, because Auth
/// Core's org posture can change between two mints a second apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRejection {
    Status(StatusCode),
    ZdrPostureMismatch,
}

impl From<StatusCode> for AuthRejection {
    fn from(status: StatusCode) -> Self {
        Self::Status(status)
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Status(status) => status.into_response(),
            Self::ZdrPostureMismatch => (
                StatusCode::FORBIDDEN,
                Json(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH.json_body()),
            )
                .into_response(),
        }
    }
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
pub async fn require_auth(mut req: Request, next: Next) -> Result<Response, AuthRejection> {
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
        return Err(StatusCode::UNAUTHORIZED.into());
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
        let sandbox_bearer = verify_delegated_sandbox_bearer(req.headers(), &claims).await?;
        let quarry_bearer = verify_delegated_quarry_bearer(req.headers(), &claims).await?;
        req.extensions_mut().insert(claims);
        req.extensions_mut()
            .insert(VerifiedModelBearer::new(&token));
        req.extensions_mut().insert(VerifiedStepUp::default());
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
        if let Some(sandbox_bearer) = sandbox_bearer {
            req.extensions_mut().insert(sandbox_bearer);
        }
        if let Some(quarry_bearer) = quarry_bearer {
            req.extensions_mut().insert(quarry_bearer);
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
        return Err(StatusCode::UNAUTHORIZED.into());
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

    let decoded = decode::<ModelTokenPayload>(&token, &decoding_key, &validation).map_err(|e| {
        warn!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;
    let step_up = VerifiedStepUp(decoded.claims.step_up);
    let token_data = TokenData {
        header: decoded.header,
        claims: decoded.claims.identity,
    };

    if token_data.claims.org_id.is_empty() {
        warn!("JWT claims missing org_id");
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    let principal_kind = token_data.claims.principal_kind().inspect_err(|_| {
        warn!("JWT carries missing, mismatched, or ambiguous principal identity");
    })?;
    let mut api_key_browser_bearer = None;
    let mut api_key_session_bearer = None;
    let mut api_key_inference_bearer = None;
    let (
        data_plane_bearer,
        capability_bearer,
        cost_bearer,
        session_bearer,
        inference_bearer,
        execution_bearer,
        browser_bearer,
        ingestion_bearer,
        sandbox_bearer,
        quarry_bearer,
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
            verify_delegated_sandbox_bearer(req.headers(), &token_data.claims).await?,
            verify_delegated_quarry_bearer(req.headers(), &token_data.claims).await?,
        ),
        PrincipalKind::Service => {
            if req.headers().contains_key("x-data-plane-authorization")
                || req.headers().contains_key("x-capability-authorization")
                || req.headers().contains_key("x-cost-authorization")
                || req.headers().contains_key("x-execution-authorization")
                || req.headers().contains_key("x-browser-authorization")
                || req.headers().contains_key("x-ingestion-authorization")
                || req.headers().contains_key("x-sandbox-authorization")
                || req.headers().contains_key("x-quarry-authorization")
                || (req.headers().contains_key("x-session-authorization")
                    && req.headers().contains_key("x-inference-authorization"))
            {
                warn!(
                    "service principal supplied an unauthorized or ambiguous downstream delegation"
                );
                return Err(StatusCode::FORBIDDEN.into());
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
                None,
                None,
            )
        }
        PrincipalKind::ApiKey => {
            // A key carries no user identity, so no user-bound downstream bearer can be bound to
            // it. Only the key's own broker bearer, on the key-grant routes, and (EP-12 S5) the
            // key's own session bearer, on an Agent API route while its switch is on, and (S7) its
            // own inference bearer, on the start route only.
            let on_agent_api_route = agent_api_route(req.method().as_str(), req.uri().path())
                && crate::agent_api_routes::enabled();
            let on_agent_api_start = on_agent_api_route
                && agent_api_start_route(req.method().as_str(), req.uri().path());
            if API_KEY_FORBIDDEN_DELEGATIONS.iter().any(|header| {
                req.headers().contains_key(*header)
                    && !(on_agent_api_route && *header == API_KEY_SESSION_HEADER)
                    && !(on_agent_api_start && *header == API_KEY_INFERENCE_HEADER)
            }) {
                warn!("api key principal supplied a downstream delegation it may not hold");
                return Err(StatusCode::FORBIDDEN.into());
            }
            api_key_browser_bearer = verify_delegated_api_key_browser_bearer(
                req.headers(),
                &token_data.claims,
                req.method().as_str(),
                req.uri().path(),
            )
            .await?;
            if on_agent_api_route {
                api_key_session_bearer =
                    verify_delegated_api_key_session_bearer(req.headers(), &token_data.claims)
                        .await?;
            }
            if on_agent_api_start {
                api_key_inference_bearer =
                    verify_delegated_api_key_inference_bearer(req.headers(), &token_data.claims)
                        .await?;
            }
            (None, None, None, None, None, None, None, None, None, None)
        }
    };
    if let Some(bearer) = api_key_browser_bearer {
        req.extensions_mut().insert(bearer);
    }
    if let Some(bearer) = api_key_session_bearer {
        req.extensions_mut().insert(bearer);
    }
    if let Some(bearer) = api_key_inference_bearer {
        req.extensions_mut().insert(bearer);
    }
    req.extensions_mut().insert(step_up);
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
    if let Some(sandbox_bearer) = sandbox_bearer {
        req.extensions_mut().insert(sandbox_bearer);
    }
    if let Some(quarry_bearer) = quarry_bearer {
        req.extensions_mut().insert(quarry_bearer);
    }
    Ok(next.run(req).await)
}

const SERVICE_SCOPE_MODELS_INVOKE: &str = "models:invoke";
const SERVICE_SCOPE_INFERENCE_INVOKE: &str = "inference:invoke";
const SERVICE_SCOPE_MODEL_SPACE_DELETE: &str = "model:space-delete";
const CONTROL_SPACE_DELETION_SERVICE: &str = "service:control-space-deletion";
/// Delegated-identity design, Gap B (§2.4(ii)): the ONLY caller allowed to
/// reach the internal mail-task thread-prepare bridge. The gateway BFF
/// (Frontend Plane, `verevon-gateway`) mints this via auth-core's existing
/// `POST /api/model-plane/internal-token`, the same way it already
/// authenticates to audit-core/org-core/billing-core.
const MAIL_TASK_BRIDGE_CALLER_SERVICE: &str = "service:verevon-gateway";
const SERVICE_SCOPE_MAIL_TASK_PREPARE_THREAD: &str = "mail-task:prepare-thread";
/// EP-7 S6: the one caller that asks for the completion judge of an unattended goal fire,
/// orchestrator-core, which runs the scheduled fire (owner decision D9). Only this principal holds
/// the judge scope.
const SERVICE_GOAL_JUDGE_CALLER: &str = "service:orchestrator-core";
const SERVICE_SCOPE_MODEL_GOAL_JUDGE: &str = "model:goal_judge";
/// EP-7 S6: the judge route's path, written as the router names it (`:goal_id` is one segment).
pub const GOAL_JUDGE_TURN_PATH: &str = "/internal/v1/goals/:goal_id/judge-turn";

/// One row of the service route table: a service principal may call `method path` when it holds
/// `scope` and, if `service` is set, is exactly that service. `service: None` means any service
/// holding the scope.
///
/// `flag` (E3.6-8) names an environment switch that must be `1`, `true` or `on` for the SERVICE
/// lane of this route to exist: while it is off the gateway answers a service that holds the
/// scope with `404`, as if the row were not there. The route itself, and what users may do with
/// it, are unchanged by the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRouteGrant {
    pub service: Option<&'static str>,
    pub method: &'static str,
    pub path: &'static str,
    pub scope: &'static str,
    pub flag: Option<&'static str>,
}

/// E3.6-8: the service lane of `POST /v1/ai/language/pii` (the optional model tier of the
/// document PII stage). Default off.
pub const SERVICE_FLAG_AI_PII: &str = "MODEL_GATEWAY_AI_PII_SERVICE";
/// E3.6-8: the service lane of `POST /v1/ai/documents/layout` (OCR / layout). Default off.
pub const SERVICE_FLAG_AI_LAYOUT: &str = "MODEL_GATEWAY_AI_LAYOUT_SERVICE";

/// Every route a SERVICE principal may reach (WS16.S1). Users keep the existing route policy.
/// This table is the single source of the machine-route policy and is pinned by the golden
/// `api/contracts/model-gateway-service-routes.json`: a route added here changes the golden in
/// the same change, which is what lets another plane's fake gateway read the real rules.
pub const SERVICE_ROUTE_GRANTS: &[ServiceRouteGrant] = &[
    ServiceRouteGrant {
        service: None,
        method: "POST",
        path: "/v1/ai/chat",
        scope: SERVICE_SCOPE_MODELS_INVOKE,
        flag: None,
    },
    ServiceRouteGrant {
        service: None,
        method: "POST",
        path: "/v1/ai/embeddings",
        scope: SERVICE_SCOPE_MODELS_INVOKE,
        flag: None,
    },
    ServiceRouteGrant {
        service: Some(CONTROL_SPACE_DELETION_SERVICE),
        method: "POST",
        path: "/v1/internal/space-deletion/threads",
        scope: SERVICE_SCOPE_MODEL_SPACE_DELETE,
        flag: None,
    },
    ServiceRouteGrant {
        service: Some(MAIL_TASK_BRIDGE_CALLER_SERVICE),
        method: "POST",
        path: "/v1/internal/mail-task/prepare-thread",
        scope: SERVICE_SCOPE_MAIL_TASK_PREPARE_THREAD,
        flag: None,
    },
    // EP-7 S6: orchestrator-core asks for the completion judge of one unattended goal fire. The route
    // answers 404 while its two switches are off (goal_unattended), so this row carries no flag.
    ServiceRouteGrant {
        service: Some(SERVICE_GOAL_JUDGE_CALLER),
        method: "POST",
        path: GOAL_JUDGE_TURN_PATH,
        scope: SERVICE_SCOPE_MODEL_GOAL_JUDGE,
        flag: None,
    },
    // E3.6-8: the two routes below exist for a service only while their switch is on.
    ServiceRouteGrant {
        service: None,
        method: "POST",
        path: "/v1/ai/documents/layout",
        scope: SERVICE_SCOPE_MODELS_INVOKE,
        flag: Some(SERVICE_FLAG_AI_LAYOUT),
    },
    ServiceRouteGrant {
        service: None,
        method: "POST",
        path: "/v1/ai/language/pii",
        scope: SERVICE_SCOPE_MODELS_INVOKE,
        flag: Some(SERVICE_FLAG_AI_PII),
    },
];

/// The second bearer a service call to `/v1/ai/chat` needs, as the golden records it.
pub const SERVICE_INFERENCE_BEARER_HEADER: &str = "x-inference-authorization";
const SERVICE_INFERENCE_BEARER_AUDIENCE: &str = "inference-core";

/// Whether a route-table `template` names the request `path`. A `:name` segment of the template
/// matches one non-empty segment of the path; every other segment must be equal, so a template with
/// no parameter matches exactly what it always did.
fn route_path_matches(template: &str, path: &str) -> bool {
    let mut template_segments = template.split('/');
    let mut path_segments = path.split('/');
    loop {
        match (template_segments.next(), path_segments.next()) {
            (None, None) => return true,
            (Some(expected), Some(actual)) => {
                let segment_ok = if expected.starts_with(':') {
                    !actual.is_empty()
                } else {
                    expected == actual
                };
                if !segment_ok {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Whether `claims`, a service principal, may call `method path`.
fn service_route_allowed(claims: &Claims, method: &str, path: &str) -> bool {
    SERVICE_ROUTE_GRANTS.iter().any(|grant| {
        grant.method == method
            && route_path_matches(grant.path, path)
            && grant.service.is_none_or(|service| claims.sub == service)
            && claims.has_scope(grant.scope)
    })
}

/// Whether a switch value turns a flag on: only the literals `1`, `true` and `on`.
fn service_flag_on(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "on"))
}

/// Whether `claims` holds a row for `method path` whose switch is OFF, so the route does not
/// exist for it yet. `lookup` reads the environment (injected for tests). A row without a
/// switch, a row the caller does not hold, and a route in no row all answer `false`: this
/// only ever turns an admitted request into a `404`.
fn service_route_switched_off(
    claims: &Claims,
    method: &str,
    path: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> bool {
    SERVICE_ROUTE_GRANTS.iter().any(|grant| {
        grant.method == method
            && route_path_matches(grant.path, path)
            && grant.service.is_none_or(|service| claims.sub == service)
            && claims.has_scope(grant.scope)
            && grant
                .flag
                .is_some_and(|flag| !service_flag_on(lookup(flag).as_deref()))
    })
}

/// The route table and the second-bearer requirement as the JSON golden pins them.
#[must_use]
pub fn service_route_table_json() -> serde_json::Value {
    serde_json::json!({
        "contract": "model-gateway.service-routes",
        "version": 1,
        "generated_by": "model-gateway auth tests (MP_UPDATE_GOLDEN=1)",
        "routes": SERVICE_ROUTE_GRANTS
            .iter()
            .map(|grant| serde_json::json!({
                "service": grant.service,
                "method": grant.method,
                "path": grant.path,
                "scope": grant.scope,
                "flag": grant.flag,
            }))
            .collect::<Vec<_>>(),
        "service_inference_bearer": {
            "header": SERVICE_INFERENCE_BEARER_HEADER,
            "audience": SERVICE_INFERENCE_BEARER_AUDIENCE,
            "requires_scopes": [SERVICE_SCOPE_MODELS_INVOKE],
            "downstream_requires_scope": SERVICE_SCOPE_INFERENCE_INVOKE,
        },
    })
}

/// Restrict service principals to the rows of [`SERVICE_ROUTE_GRANTS`].
/// Users retain the existing route policy. This must run after [`require_auth`].
///
/// A service call to `POST /v1/ai/chat` needs TWO bearers: the gateway token
/// (`Authorization`, `aud=model-gateway`, scope `models:invoke`) and a second
/// `x-inference-authorization` (`aud=inference-core`, scope `inference:invoke`), verified by
/// `verify_delegated_service_inference_bearer`, which also requires its `sub` and `service_id`
/// to equal the ingress token's. The ingress claims must pass [`Claims::principal_kind`] as a
/// service: `service_id == sub == "service:<id>"` and a `reason` of 3 to 500 characters.
///
/// # Errors
/// Returns `401` when verified claims are absent/ambiguous and `403` when a
/// service principal lacks the exact route or the row's scope.
pub async fn authorize_principal_route(req: Request, next: Next) -> Result<Response, StatusCode> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    match claims.principal_kind()? {
        PrincipalKind::User => Ok(next.run(req).await),
        PrincipalKind::Service => {
            let allowed = service_route_allowed(claims, req.method().as_str(), req.uri().path());
            if !allowed {
                warn!(
                    service = %claims.sub,
                    method = %req.method(),
                    path = %req.uri().path(),
                    "service principal route or scope denied"
                );
                return Err(StatusCode::FORBIDDEN);
            }
            // E3.6-8: a switched-off lane is a 404, after the scope check so a caller without
            // the scope learns nothing about it (it gets the same 403 as any other route).
            if service_route_switched_off(claims, req.method().as_str(), req.uri().path(), &|name| {
                std::env::var(name).ok()
            }) {
                return Err(StatusCode::NOT_FOUND);
            }
            Ok(next.run(req).await)
        }
        // INNH-10: a key reaches exactly the key-grant routes, with `browser:agent`, and (EP-12 S5)
        // the Agent API routes, with `agent:runs`, while `MODEL_GATEWAY_AGENT_API` is on. Every
        // other route (and every route added later) is a 403: this arm is an allow-list, not a
        // deny-list. The switch is read only for a path that is an Agent API route.
        PrincipalKind::ApiKey => {
            let (method, path) = (req.method().as_str(), req.uri().path());
            let agent_api = agent_api_route(method, path) && crate::agent_api_routes::enabled();
            let scope = api_key_route_scope(method, path, agent_api);
            if scope.is_some_and(|scope| claims.has_scope(scope)) {
                Ok(next.run(req).await)
            } else if scope == Some(API_KEY_SCOPE_AGENT_RUNS) {
                // The Agent API contract names this refusal, so a client can tell a key that
                // lacks the permission from a route it may not reach.
                Ok(crate::agent_api_routes::scope_missing().into_response())
            } else {
                warn!(
                    method = %req.method(),
                    path = %req.uri().path(),
                    "api key principal route or scope denied"
                );
                Err(StatusCode::FORBIDDEN)
            }
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
mod step_up_payload_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_step_up_claim_is_read_beside_the_identity_it_belongs_to() {
        let identity = json!({
            "sub": "user-1",
            "iss": "auth-core",
            "exp": 1_900_000_000_i64,
            "org_id": "org-1",
            "user_id": "user-1",
            "zdr": false,
        });
        let mut stepped_up = identity.clone();
        stepped_up["step_up"] = json!({ "at": 1_800_000_000_i64, "method": "password" });

        let parsed: ModelTokenPayload =
            serde_json::from_value(stepped_up).expect("a stepped-up token payload parses");
        assert_eq!(
            parsed.step_up,
            Some(StepUpClaim {
                at: 1_800_000_000,
                method: "password".to_owned(),
            })
        );
        assert_eq!(parsed.identity.org_id, "org-1");

        let plain: ModelTokenPayload =
            serde_json::from_value(identity).expect("a payload without the claim parses");
        assert_eq!(plain.step_up, None);
        assert_eq!(plain.identity.user_id, "user-1");
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
        std::env::remove_var("SANDBOX_MANAGER_AUTH_AUDIENCE");
        std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
        std::env::remove_var("ALLOW_INSECURE_DEV_DEFAULTS");
        std::env::remove_var(crate::agent_api_routes::ENABLED_ENV);
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
            key_id: None,
            acting_user_id: None,
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: 1_700_000_000,
            org_id: "o1".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
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
            key_id: None,
            acting_user_id: None,
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: 1_700_000_000,
            org_id: "o1".into(),
            user_id: "u1".into(),
            nbf: None,
            aud: Some("model-gateway".into()),
            scopes: vec!["models:read".into(), "models:invoke".into()],
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
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
            key_id: None,
            acting_user_id: None,
            sub: "user-test".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "org-a".into(),
            user_id: "u".into(),
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
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
    async fn delegated_service_inference_bearer_requires_its_own_scope_and_identity() {
        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-service-inference-scope";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");

        let model_claims = Claims {
            sub: "service:studio-worker".into(),
            user_id: String::new(),
            principal_type: Some("service".into()),
            service_id: Some("service:studio-worker".into()),
            reason: Some("analyze approved Studio evidence".into()),
            scopes: vec!["models:invoke".into()],
            ..base_claims()
        };
        let inference_claims = Claims {
            aud: Some("inference-core".into()),
            scopes: vec!["inference:invoke".into()],
            ..model_claims.clone()
        };

        let mut headers = HeaderMap::new();
        let token = sign_jwt(&inference_claims, kid);
        headers.insert(
            "x-inference-authorization",
            format!("Bearer {token}").parse().unwrap(),
        );
        let verified = verify_delegated_service_inference_bearer(&headers, &model_claims)
            .await
            .expect("the inference audience's invoke scope must be accepted")
            .expect("the delegated bearer is present");
        assert_eq!(verified.as_str(), token);

        let invalid = [
            (
                Claims {
                    scopes: vec!["models:invoke".into()],
                    ..inference_claims.clone()
                },
                StatusCode::FORBIDDEN,
            ),
            (
                Claims {
                    scopes: vec![],
                    ..inference_claims.clone()
                },
                StatusCode::FORBIDDEN,
            ),
            (
                Claims {
                    org_id: "other-org".into(),
                    ..inference_claims.clone()
                },
                StatusCode::FORBIDDEN,
            ),
            (
                Claims {
                    sub: "service:other-worker".into(),
                    service_id: Some("service:other-worker".into()),
                    ..inference_claims.clone()
                },
                StatusCode::FORBIDDEN,
            ),
            (
                Claims {
                    aud: Some("model-gateway".into()),
                    ..inference_claims.clone()
                },
                StatusCode::UNAUTHORIZED,
            ),
        ];
        for (claims, expected) in invalid {
            headers.insert(
                "x-inference-authorization",
                format!("Bearer {}", sign_jwt(&claims, kid))
                    .parse()
                    .unwrap(),
            );
            assert_eq!(
                verify_delegated_service_inference_bearer(&headers, &model_claims)
                    .await
                    .unwrap_err(),
                expected,
            );
        }
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

    fn api_key_claims(key_id: &str) -> Claims {
        Claims {
            sub: format!("apikey:{key_id}"),
            user_id: String::new(),
            principal_type: Some("api_key".to_owned()),
            key_id: Some(key_id.to_owned()),
            acting_user_id: Some("user-test".to_owned()),
            scopes: vec![API_KEY_SCOPE_BROWSER_AGENT.to_owned()],
            ..base_claims()
        }
    }

    #[test]
    fn api_key_principal_shape_is_exact_and_the_key_id_is_refused_elsewhere() {
        assert_eq!(api_key_claims("key-1").principal_kind(), Ok(PrincipalKind::ApiKey));
        assert_eq!(api_key_claims("key-1").api_key_id(), Some("key-1"));
        let refused: Vec<(&str, Claims)> = vec![
            ("sub mismatch", Claims { sub: "apikey:key-2".into(), ..api_key_claims("key-1") }),
            ("sub without prefix", Claims { sub: "key-1".into(), ..api_key_claims("key-1") }),
            ("missing key_id", Claims { key_id: None, ..api_key_claims("key-1") }),
            ("empty key_id", Claims { sub: "apikey:".into(), ..api_key_claims("") }),
            ("key_id over 128", api_key_claims(&"k".repeat(129))),
            ("key_id with a space", api_key_claims("key 1")),
            ("missing owner", Claims { acting_user_id: None, ..api_key_claims("key-1") }),
            ("empty owner", Claims { acting_user_id: Some(String::new()), ..api_key_claims("key-1") }),
            ("padded owner", Claims { acting_user_id: Some(" user".into()), ..api_key_claims("key-1") }),
            ("owner is a key", Claims { acting_user_id: Some("apikey:k".into()), ..api_key_claims("key-1") }),
            ("user_id present", Claims { user_id: "user-test".into(), ..api_key_claims("key-1") }),
            ("service_id present", Claims { service_id: Some("service:x".into()), ..api_key_claims("key-1") }),
            ("key_id on a user", Claims { key_id: Some("key-1".into()), ..base_claims() }),
            (
                "key_id on a service",
                Claims {
                    sub: "service:quarry".into(),
                    user_id: String::new(),
                    principal_type: Some("service".into()),
                    service_id: Some("service:quarry".into()),
                    reason: Some("test service".into()),
                    key_id: Some("key-1".into()),
                    ..base_claims()
                },
            ),
        ];
        for (case, claims) in refused {
            assert_eq!(claims.principal_kind(), Err(StatusCode::UNAUTHORIZED), "{case}");
            assert_eq!(claims.api_key_id(), None, "{case}");
        }
        assert_eq!(base_claims().principal_kind(), Ok(PrincipalKind::User), "users unchanged");
    }

    #[test]
    fn api_key_routes_are_exactly_the_two_key_grant_shapes() {
        assert!(api_key_route_allowed("POST", "/v1/browser/key-grants"));
        assert!(api_key_route_allowed("DELETE", "/v1/browser/key-grants/abc"));
        for (method, path) in [
            ("GET", "/v1/browser/key-grants"),
            ("DELETE", "/v1/browser/key-grants"),
            ("DELETE", "/v1/browser/key-grants/"),
            ("DELETE", "/v1/browser/key-grants/a/b"),
            ("POST", "/v1/browser/key-grants/abc"),
            ("POST", "/v1/browser/key-grants/abc/renew"),
            ("POST", "/v1/browser/grants"),
            ("DELETE", "/v1/browser/grants/abc"),
            ("POST", "/v1/browser/key-grants-x"),
            ("POST", "/v1/invoke"),
        ] {
            assert!(!api_key_route_allowed(method, path), "{method} {path}");
        }
        assert_eq!(API_KEY_ROUTES.len(), 2);
    }

    /// EP-12 S5: the key allow-list as a table over the Agent API switch. Off, it is exactly the
    /// INNH-10 pair with `browser:agent`; on, the six Agent API shapes join it with
    /// `agent:runs`, and nothing else does.
    #[test]
    fn api_key_route_scope_is_a_table_over_the_agent_api_switch() {
        let key_grant = Some(API_KEY_SCOPE_BROWSER_AGENT);
        let agent = Some(API_KEY_SCOPE_AGENT_RUNS);
        let table: &[(&str, &str, Option<&str>, Option<&str>)] = &[
            // (method, path, switch off, switch on)
            ("POST", "/v1/browser/key-grants", key_grant, key_grant),
            ("DELETE", "/v1/browser/key-grants/g1", key_grant, key_grant),
            ("POST", "/v1/agent/runs", None, agent),
            ("GET", "/v1/agent/runs/run-1", None, agent),
            ("GET", "/v1/agent/runs/run-1/events", None, agent),
            ("POST", "/v1/agent/runs/run-1/cancel", None, agent),
            ("GET", "/v1/agent/threads/thread-1/runs", None, agent),
            ("GET", "/v1/agent/threads/thread-1/messages", None, agent),
            // Wrong method, missing or extra segment, trailing slash, the people's run routes.
            ("POST", "/v1/agent/runs/run-1", None, None),
            ("DELETE", "/v1/agent/runs/run-1", None, None),
            ("GET", "/v1/agent/runs/run-1/cancel", None, None),
            ("GET", "/v1/agent/runs", None, None),
            ("PUT", "/v1/agent/runs", None, None),
            ("POST", "/v1/agent/runs/", None, None),
            ("GET", "/v1/agent/runs/", None, None),
            ("GET", "/v1/agent/runs//events", None, None),
            ("GET", "/v1/agent/runs/run-1/", None, None),
            ("GET", "/v1/agent/runs/run-1/events/replay", None, None),
            ("GET", "/v1/agent/runs/a/b", None, None),
            ("GET", "/v1/agent/threads/thread-1", None, None),
            ("GET", "/v1/agent/threads/thread-1/events", None, None),
            ("GET", "/v1/agent/threads/thread-1/messages/x", None, None),
            ("GET", "/v1/runs/run-1", None, None),
            ("GET", "/v1/runs/run-1/events", None, None),
            ("POST", "/v1/runs/run-1/cancel", None, None),
            ("GET", "/v1/threads/thread-1/messages", None, None),
            ("POST", "/v1/invoke", None, None),
        ];
        for (method, path, off, on) in table {
            assert_eq!(
                api_key_route_scope(method, path, false),
                *off,
                "off: {method} {path}"
            );
            assert_eq!(
                api_key_route_scope(method, path, true),
                *on,
                "on: {method} {path}"
            );
        }
        assert_eq!(AGENT_API_KEY_ROUTES.len(), 6);
        for (method, pattern) in AGENT_API_KEY_ROUTES {
            let concrete = pattern.replace(":run_id", "r1").replace(":thread_id", "t1");
            assert_eq!(
                api_key_route_scope(method, &concrete, true),
                agent,
                "{method} {pattern}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn authorize_principal_route_admits_a_key_on_the_agent_api_only_while_it_is_on() {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        async fn call(claims: Claims, method: &str, path: &str) -> (StatusCode, String) {
            let response = Router::new()
                .route("/v1/agent/runs/:run_id", get(ok_handler))
                .route(
                    "/v1/agent/runs/:run_id/cancel",
                    axum::routing::post(ok_handler),
                )
                .route("/v1/browser/key-grants", axum::routing::post(ok_handler))
                .route("/v1/runs/:run_id", get(ok_handler))
                .layer(middleware::from_fn(authorize_principal_route))
                .layer(axum::Extension(claims))
                .oneshot(
                    HttpRequest::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 1 << 16)
                .await
                .unwrap();
            (status, String::from_utf8_lossy(&body).into_owned())
        }
        let agent_key = Claims {
            scopes: vec![API_KEY_SCOPE_AGENT_RUNS.to_owned()],
            ..api_key_claims("key-1")
        };

        clear_env();
        let (status, body) = call(agent_key.clone(), "GET", "/v1/agent/runs/r1").await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "off: the agent routes are not on the list"
        );
        assert!(body.is_empty(), "off: the bare 403 every other route gives");

        for value in ["1", "on", "yes", "truthy", ""] {
            std::env::set_var(crate::agent_api_routes::ENABLED_ENV, value);
            let (status, _) = call(agent_key.clone(), "GET", "/v1/agent/runs/r1").await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "only the word true enables it: {value:?}"
            );
        }

        std::env::set_var(crate::agent_api_routes::ENABLED_ENV, "true");
        assert_eq!(
            call(agent_key.clone(), "GET", "/v1/agent/runs/r1").await.0,
            StatusCode::OK
        );
        assert_eq!(
            call(agent_key.clone(), "POST", "/v1/agent/runs/r1/cancel")
                .await
                .0,
            StatusCode::OK
        );
        // `agent:runs` opens neither the key-grant routes nor a person's run route.
        assert_eq!(
            call(agent_key.clone(), "POST", "/v1/browser/key-grants")
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(agent_key.clone(), "GET", "/v1/runs/r1").await.0,
            StatusCode::FORBIDDEN
        );
        // A key without `agent:runs` gets the contract's typed refusal, naming the scope.
        let (status, body) = call(api_key_claims("key-1"), "GET", "/v1/agent/runs/r1").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: serde_json::Value = serde_json::from_str(&body).expect("a typed body");
        assert_eq!(body["error"]["code"], "agent_api_scope_missing");
        assert_eq!(body["error"]["details"]["scope"], API_KEY_SCOPE_AGENT_RUNS);
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_api_key_session_bearer_is_the_same_key_once_and_only_on_agent_routes() {
        async fn echo_key_session(
            bearer: Option<axum::Extension<VerifiedApiKeySessionBearer>>,
        ) -> String {
            bearer.map_or_else(String::new, |bearer| {
                bearer.0.as_session_bearer().as_str().to_owned()
            })
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-api-key-session-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");

        let agent = |claims: Claims| Claims {
            scopes: vec![API_KEY_SCOPE_AGENT_RUNS.to_owned()],
            ..claims
        };
        let session_aud = |claims: Claims| Claims {
            aud: Some("session-core".into()),
            ..agent(claims)
        };
        let model_token = sign_jwt(&agent(api_key_claims("key-1")), kid);
        let key_session = sign_jwt(&session_aud(api_key_claims("key-1")), kid);
        let router = || {
            Router::new()
                .route("/v1/agent/runs/:run_id", get(echo_key_session))
                .route("/v1/runs/:run_id", get(echo_key_session))
                .layer(middleware::from_fn(require_auth))
        };
        let call = |path: &'static str, extra: Vec<(&'static str, String)>| {
            let mut builder = HttpRequest::builder()
                .method("GET")
                .uri(path)
                .header("authorization", format!("Bearer {model_token}"));
            for (name, value) in extra {
                builder = builder.header(name, value);
            }
            router().oneshot(builder.body(Body::empty()).unwrap())
        };
        let session_header =
            |token: &str| vec![("x-session-authorization", format!("Bearer {token}"))];

        // Off: the header is one a key may not send, as before.
        let response = call("/v1/agent/runs/r1", session_header(&key_session))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "switch off");

        std::env::set_var(crate::agent_api_routes::ENABLED_ENV, "true");
        let response = call("/v1/agent/runs/r1", session_header(&key_session))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        assert_eq!(
            body,
            key_session.as_bytes(),
            "the key's own session bearer is forwarded"
        );

        // The same header on any other route stays forbidden.
        let response = call("/v1/runs/r1", session_header(&key_session))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "a person's run route"
        );

        // Twice, even the same value: refused.
        let response = call(
            "/v1/agent/runs/r1",
            vec![
                ("x-session-authorization", format!("Bearer {key_session}")),
                ("x-session-authorization", format!("Bearer {key_session}")),
            ],
        )
        .await
        .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "exactly one session bearer"
        );

        let unscoped = sign_jwt(
            &Claims {
                aud: Some("session-core".into()),
                ..api_key_claims("key-1")
            },
            kid,
        );
        let another_key = sign_jwt(&session_aud(api_key_claims("key-2")), kid);
        let another_owner = sign_jwt(
            &session_aud(Claims {
                acting_user_id: Some("user-other".into()),
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let other_org = sign_jwt(
            &session_aud(Claims {
                org_id: "org-other".into(),
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let a_user = sign_jwt(
            &Claims {
                aud: Some("session-core".into()),
                ..base_claims()
            },
            kid,
        );
        for (case, token) in [
            ("without agent:runs", unscoped),
            ("another key", another_key),
            ("another owner", another_owner),
            ("another org", other_org),
            ("a user bearer", a_user),
        ] {
            let response = call("/v1/agent/runs/r1", session_header(&token))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{case}");
        }

        let wrong_audience = sign_jwt(&agent(api_key_claims("key-1")), kid);
        let response = call("/v1/agent/runs/r1", session_header(&wrong_audience))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "the model token is not a session bearer"
        );

        let zdr_session = sign_jwt(
            &session_aud(Claims {
                zdr: true,
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let response = call("/v1/agent/runs/r1", session_header(&zdr_session))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("zdr_posture_mismatch"),
            "a mixed posture is the typed refusal"
        );

        // Every other downstream bearer stays forbidden on an Agent API route.
        for header in API_KEY_FORBIDDEN_DELEGATIONS
            .iter()
            .filter(|header| **header != API_KEY_SESSION_HEADER)
        {
            let response = call(
                "/v1/agent/runs/r1",
                vec![
                    ("x-session-authorization", format!("Bearer {key_session}")),
                    (*header, format!("Bearer {key_session}")),
                ],
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{header}");
        }
        let response = call(
            "/v1/agent/runs/r1",
            vec![("x-browser-authorization", format!("Bearer {key_session}"))],
        )
        .await
        .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "the broker bearer stays on the key-grant routes"
        );

        // Without the header the request passes auth with no key session bearer for the handler.
        let response = call("/v1/agent/runs/r1", Vec::new()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(body.is_empty());
        clear_env();
    }

    /// EP-12 S7: a key's own `aud=inference-core` bearer passes only on the start route, while
    /// the switch is on, once, and only as the same key holding `agent:runs`.
    #[tokio::test]
    #[serial]
    async fn delegated_api_key_inference_bearer_is_the_same_key_only_on_the_start_route() {
        async fn echo_key_inference(
            bearer: Option<axum::Extension<VerifiedApiKeyInferenceBearer>>,
        ) -> String {
            bearer.map_or_else(String::new, |bearer| bearer.0.as_str().to_owned())
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-api-key-inference-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");
        std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");

        let agent = |claims: Claims| Claims {
            scopes: vec![API_KEY_SCOPE_AGENT_RUNS.to_owned()],
            ..claims
        };
        let inference_aud = |claims: Claims| Claims {
            aud: Some("inference-core".into()),
            ..agent(claims)
        };
        let model_token = sign_jwt(&agent(api_key_claims("key-1")), kid);
        let key_inference = sign_jwt(&inference_aud(api_key_claims("key-1")), kid);
        let router = || {
            Router::new()
                .route("/v1/agent/runs", axum::routing::post(echo_key_inference))
                .route("/v1/agent/runs/:run_id", get(echo_key_inference))
                .layer(middleware::from_fn(require_auth))
        };
        let call = |method: &'static str, path: &'static str, token: &str| {
            router().oneshot(
                HttpRequest::builder()
                    .method(method)
                    .uri(path)
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-inference-authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
        };

        let response = call("POST", "/v1/agent/runs", &key_inference)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "switch off");

        std::env::set_var(crate::agent_api_routes::ENABLED_ENV, "true");
        let response = call("POST", "/v1/agent/runs", &key_inference)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        assert_eq!(
            body,
            key_inference.as_bytes(),
            "the key's own inference bearer is forwarded"
        );

        // Every other Agent API route keeps it forbidden: only the start route calls a model.
        let response = call("GET", "/v1/agent/runs/r1", &key_inference)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "a read route");

        let response = router()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/v1/agent/runs")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-inference-authorization",
                        format!("Bearer {key_inference}"),
                    )
                    .header(
                        "x-inference-authorization",
                        format!("Bearer {key_inference}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "exactly one inference bearer"
        );

        let unscoped = sign_jwt(
            &Claims {
                aud: Some("inference-core".into()),
                ..api_key_claims("key-1")
            },
            kid,
        );
        let another_key = sign_jwt(&inference_aud(api_key_claims("key-2")), kid);
        let a_user = sign_jwt(
            &Claims {
                aud: Some("inference-core".into()),
                ..base_claims()
            },
            kid,
        );
        for (case, token) in [
            ("without agent:runs", unscoped),
            ("another key", another_key),
            ("a user bearer", a_user),
        ] {
            let response = call("POST", "/v1/agent/runs", &token).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{case}");
        }

        let session_audience = sign_jwt(
            &Claims {
                aud: Some("session-core".into()),
                ..agent(api_key_claims("key-1"))
            },
            kid,
        );
        let response = call("POST", "/v1/agent/runs", &session_audience)
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a session bearer is not an inference bearer"
        );

        let zdr_inference = sign_jwt(
            &inference_aud(Claims {
                zdr: true,
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let response = call("POST", "/v1/agent/runs", &zdr_inference)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("zdr_posture_mismatch"),
            "a mixed posture is the typed refusal"
        );
        clear_env();
    }

    #[tokio::test]
    async fn authorize_principal_route_admits_a_key_only_on_its_routes_with_its_scope() {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        async fn status_of(claims: Claims, method: &str, path: &str) -> StatusCode {
            Router::new()
                .route("/v1/browser/key-grants", axum::routing::post(ok_handler))
                .route("/v1/browser/key-grants/:id", axum::routing::delete(ok_handler))
                .route("/v1/invoke", axum::routing::post(ok_handler))
                .layer(middleware::from_fn(authorize_principal_route))
                .layer(axum::Extension(claims))
                .oneshot(
                    HttpRequest::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
        let key = api_key_claims("key-1");
        assert_eq!(status_of(key.clone(), "POST", "/v1/browser/key-grants").await, StatusCode::OK);
        assert_eq!(status_of(key.clone(), "DELETE", "/v1/browser/key-grants/g1").await, StatusCode::OK);
        assert_eq!(status_of(key.clone(), "POST", "/v1/invoke").await, StatusCode::FORBIDDEN);
        let unscoped = Claims { scopes: Vec::new(), ..key };
        assert_eq!(
            status_of(unscoped, "POST", "/v1/browser/key-grants").await,
            StatusCode::FORBIDDEN,
            "a key without browser:agent is refused even on its route"
        );
        // A user passes the fence on the key routes (the handler refuses it), as before.
        assert_eq!(status_of(base_claims(), "POST", "/v1/invoke").await, StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn delegated_api_key_bearer_must_be_the_same_key_and_only_on_the_key_routes() {
        async fn echo_key_bearer(bearer: VerifiedApiKeyBrowserBearer) -> String {
            bearer.as_str().to_owned()
        }
        async fn echo_user_bearer(bearer: VerifiedBrowserBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-api-key-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("BROWSER_BROKER_AUTH_AUDIENCE", "browser-broker");

        let broker_aud = |claims: Claims| Claims {
            aud: Some("browser-broker".into()),
            ..claims
        };
        let model_token = sign_jwt(&api_key_claims("key-1"), kid);
        let key_bearer = sign_jwt(&broker_aud(api_key_claims("key-1")), kid);
        let router = || {
            Router::new()
                .route("/v1/browser/key-grants", axum::routing::post(echo_key_bearer))
                .route("/v1/browser/grants", axum::routing::post(echo_user_bearer))
                .route("/", get(echo_key_bearer))
                .layer(middleware::from_fn(require_auth))
        };
        let call = |path: &'static str, method: &'static str, extra: Vec<(&'static str, String)>| {
            let mut builder = HttpRequest::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {model_token}"));
            for (name, value) in extra {
                builder = builder.header(name, value);
            }
            router().oneshot(builder.body(Body::empty()).unwrap())
        };

        let response = call(
            "/v1/browser/key-grants",
            "POST",
            vec![("x-browser-authorization", format!("Bearer {key_bearer}"))],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
        assert_eq!(body, key_bearer.as_bytes(), "the key's own broker bearer is forwarded");

        // The same pair on any other route: refused before a handler runs.
        for (path, method) in [("/", "GET"), ("/v1/browser/grants", "POST")] {
            let response = call(
                path,
                method,
                vec![("x-browser-authorization", format!("Bearer {key_bearer}"))],
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }

        let another_key = sign_jwt(&broker_aud(api_key_claims("key-2")), kid);
        let another_owner = sign_jwt(
            &broker_aud(Claims {
                acting_user_id: Some("user-other".into()),
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let other_org = sign_jwt(
            &broker_aud(Claims {
                org_id: "org-other".into(),
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let a_user = sign_jwt(&broker_aud(base_claims()), kid);
        for (case, token) in [
            ("another key", another_key),
            ("another owner", another_owner),
            ("another org", other_org),
            ("a user bearer", a_user),
        ] {
            let response = call(
                "/v1/browser/key-grants",
                "POST",
                vec![("x-browser-authorization", format!("Bearer {token}"))],
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{case}");
        }

        let zdr_bearer = sign_jwt(
            &broker_aud(Claims {
                zdr: true,
                ..api_key_claims("key-1")
            }),
            kid,
        );
        let response = call(
            "/v1/browser/key-grants",
            "POST",
            vec![("x-browser-authorization", format!("Bearer {zdr_bearer}"))],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("zdr_posture_mismatch"),
            "a mixed posture is the typed refusal"
        );

        // No other downstream bearer may ride with a key, on any route.
        let response = call(
            "/v1/browser/key-grants",
            "POST",
            vec![
                ("x-browser-authorization", format!("Bearer {key_bearer}")),
                ("x-session-authorization", format!("Bearer {key_bearer}")),
            ],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // Without the broker bearer the handler's extractor has nothing: 401.
        let response = call("/v1/browser/key-grants", "POST", Vec::new()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // A user's browser bearer never becomes a key bearer.
        let user_model = sign_jwt(&base_claims(), kid);
        let user_browser = sign_jwt(&broker_aud(base_claims()), kid);
        let response = router()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/v1/browser/key-grants")
                    .header("authorization", format!("Bearer {user_model}"))
                    .header("x-browser-authorization", format!("Bearer {user_browser}"))
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
    async fn delegated_sandbox_bearer_requires_exact_audience_and_matching_identity() {
        async fn echo_sandbox_bearer(bearer: VerifiedSandboxBearer) -> String {
            bearer.as_str().to_owned()
        }

        clear_env();
        reset_jwks_cache_for_test().await;
        let kid = "test-kid-sandbox-delegation";
        let server = start_jwks_mock(kid).await;
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        std::env::set_var("SANDBOX_MANAGER_AUTH_AUDIENCE", "sandbox-manager");

        let model_token = sign_jwt(&base_claims(), kid);
        let mut sandbox_claims = base_claims();
        sandbox_claims.aud = Some("sandbox-manager".into());
        let sandbox_token = sign_jwt(&sandbox_claims, kid);

        let response = Router::new()
            .route("/", get(echo_sandbox_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-sandbox-authorization", format!("Bearer {sandbox_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // The header is optional: a turn that is not Space-scoped carries none,
        // and a handler that does not extract it must be unaffected.
        let response = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // The ingress (aud=model-gateway) token is never accepted as the
        // sandbox delegate: a Model Plane token is not reused across audiences.
        let wrong_audience = sign_jwt(&base_claims(), kid);
        let response = Router::new()
            .route("/", get(echo_sandbox_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header(
                        "x-sandbox-authorization",
                        format!("Bearer {wrong_audience}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut other_tenant = sandbox_claims;
        other_tenant.org_id = "org-other".into();
        let other_tenant = sign_jwt(&other_tenant, kid);
        let response = Router::new()
            .route("/", get(echo_sandbox_bearer))
            .layer(middleware::from_fn(require_auth))
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("authorization", format!("Bearer {model_token}"))
                    .header("x-sandbox-authorization", format!("Bearer {other_tenant}"))
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
            key_id: None,
            acting_user_id: None,
            sub: "user-test".into(),
            iss: "auth-core".into(),
            exp: now_secs() + 3600,
            org_id: "org-test".into(),
            user_id: "user-test".into(),
            nbf: None,
            aud: Some("model-gateway".into()),
            scopes: Vec::new(),
            zdr: false,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
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
                key_id: None,
                acting_user_id: None,
                sub: service_id.to_owned(),
                iss: "auth-core".to_owned(),
                exp: now_secs() + 60,
                org_id: "org-test".to_owned(),
                user_id: String::new(),
                nbf: None,
                aud: Some("model-gateway".to_owned()),
                scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                zdr: false,
                sovereign: None,
                org_zdr: None,
                zdr_waiver: None,
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

    #[tokio::test]
    async fn only_the_gateway_bff_reaches_the_internal_mail_task_bridge_route() {
        async fn ok_handler() -> &'static str {
            "ok"
        }

        fn service(service_id: &str, scopes: &[&str]) -> Claims {
            Claims {
                key_id: None,
                acting_user_id: None,
                sub: service_id.to_owned(),
                iss: "auth-core".to_owned(),
                exp: now_secs() + 60,
                org_id: "org-test".to_owned(),
                user_id: String::new(),
                nbf: None,
                aud: Some("model-gateway".to_owned()),
                scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
                zdr: false,
                sovereign: None,
                org_zdr: None,
                zdr_waiver: None,
                principal_type: Some("service".to_owned()),
                service_id: Some(service_id.to_owned()),
                reason: Some("prepare a mail-task-origin thread".to_owned()),
            }
        }

        fn app(claims: Claims) -> Router {
            Router::new()
                .route(
                    "/v1/internal/mail-task/prepare-thread",
                    axum::routing::post(ok_handler),
                )
                .layer(middleware::from_fn(authorize_principal_route))
                .layer(axum::Extension(claims))
        }

        let allowed = app(service(
            MAIL_TASK_BRIDGE_CALLER_SERVICE,
            &[SERVICE_SCOPE_MAIL_TASK_PREPARE_THREAD],
        ))
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/v1/internal/mail-task/prepare-thread")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);

        for claims in [
            // A real, correctly-scoped service credential -- just not the
            // one this route allow-lists. Holding SOME valid service
            // credential must never be enough on its own.
            service(
                "service:some-other-workload",
                &[SERVICE_SCOPE_MAIL_TASK_PREPARE_THREAD],
            ),
            service(MAIL_TASK_BRIDGE_CALLER_SERVICE, &[]),
            service(MAIL_TASK_BRIDGE_CALLER_SERVICE, &[SERVICE_SCOPE_MODELS_INVOKE]),
        ] {
            let response = app(claims)
                .oneshot(
                    HttpRequest::builder()
                        .method("POST")
                        .uri("/v1/internal/mail-task/prepare-thread")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }

    // ---------------------------------------------------------------------
    // Z-01: the zdr claim is required, and every user bearer of one turn
    // carries the ingress token's retention posture. The user body is the one
    // Auth Core really mints (tests/fixtures/auth-core, pinned by its .sha256
    // and by scripts/tests/zdr-contract-sync.test.sh).
    // ---------------------------------------------------------------------

    const AUTH_CORE_GOLDEN: &str =
        include_str!("../tests/fixtures/auth-core/quarry-token-claims.golden.json");
    const AUTH_CORE_GOLDEN_PIN: &str =
        include_str!("../tests/fixtures/auth-core/quarry-token-claims.golden.sha256");

    /// The nine delegated user bearers, each with the audience it is minted for.
    // Ten: Teach mode's `aud=quarry` bearer (F-t3-081, W5-081-c) is checked like every other one.
    const DELEGATED_USER_BEARERS: [(&str, &str); 10] = [
        ("x-data-plane-authorization", "data-plane"),
        ("x-ingestion-authorization", "ingestion"),
        ("x-capability-authorization", "capability-core"),
        ("x-cost-authorization", "cost-core"),
        ("x-session-authorization", "session-core"),
        ("x-inference-authorization", "inference-core"),
        ("x-execution-authorization", "execution-core"),
        ("x-browser-authorization", "browser-broker"),
        ("x-sandbox-authorization", "sandbox-manager"),
        ("x-quarry-authorization", "quarry"),
    ];

    /// The user claims Auth Core really minted for one golden case, with the
    /// time claims stamped the way the consumer stamps them and the audience
    /// set by the test.
    fn minted_user_claims(case: &str, audience: &str) -> serde_json::Value {
        let golden: serde_json::Value =
            serde_json::from_str(AUTH_CORE_GOLDEN).expect("golden JSON parses");
        let mut claims = golden["cases"][case]["claims"].clone();
        let now = now_secs();
        claims["aud"] = serde_json::json!(audience);
        claims["iat"] = serde_json::json!(now);
        claims["nbf"] = serde_json::json!(now - 5);
        claims["exp"] = serde_json::json!(now + 300);
        claims
    }

    fn with_zdr(mut claims: serde_json::Value, zdr: bool) -> serde_json::Value {
        claims["zdr"] = serde_json::json!(zdr);
        claims
    }

    /// Clear the environment and serve the test key under `kid` from a JWKS.
    /// The issuer is the one the golden was minted under.
    async fn configure_real_verification(kid: &str) -> wiremock::MockServer {
        clear_env();
        reset_jwks_cache_for_test().await;
        let server = start_jwks_mock(kid).await;
        let issuer = minted_user_claims("user_in_standard_org", "model-gateway")["iss"]
            .as_str()
            .expect("golden issuer")
            .to_owned();
        std::env::set_var(
            "AUTH_CORE_JWKS_URL",
            format!("{}/.well-known/jwks.json", server.uri()),
        );
        std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
        std::env::set_var("AUTH_CORE_ISSUER", issuer);
        server
    }

    /// One request through the real `require_auth` with the given headers.
    async fn send_with_headers(headers: &[(&str, String)]) -> (StatusCode, serde_json::Value) {
        let mut request = HttpRequest::builder().uri("/");
        for (name, value) in headers {
            request = request.header(*name, value.as_str());
        }
        let response = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth))
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    #[serial]
    async fn zdr_claim_is_required_for_a_user_token() {
        let kid = "test-kid-zdr-required";
        let _server = configure_real_verification(kid).await;

        let mut without_claim = minted_user_claims("user_in_standard_org", "model-gateway");
        without_claim
            .as_object_mut()
            .expect("claims object")
            .remove("zdr");
        assert_eq!(
            send_with_token(&sign_test_jwt(&without_claim, kid)).await,
            StatusCode::UNAUTHORIZED,
            "a user token without the zdr claim must not be read as persistent"
        );

        let with_claim = minted_user_claims("user_in_standard_org", "model-gateway");
        assert_eq!(
            send_with_token(&sign_test_jwt(&with_claim, kid)).await,
            StatusCode::OK,
            "the same real body with the claim present is accepted"
        );
        clear_env();
    }

    #[test]
    fn claims_struct_requires_the_zdr_field() {
        let mut without_claim = minted_user_claims("user_in_standard_org", "model-gateway");
        without_claim
            .as_object_mut()
            .expect("claims object")
            .remove("zdr");
        assert!(serde_json::from_value::<Claims>(without_claim).is_err());

        let minted: Claims =
            serde_json::from_value(minted_user_claims("user_in_zdr_org", "model-gateway"))
                .expect("the real mint parses");
        assert!(minted.zdr);
    }

    #[test]
    fn vendored_auth_core_golden_matches_its_pin_and_both_postures() {
        use sha2::{Digest, Sha256};
        let digest: String = Sha256::digest(AUTH_CORE_GOLDEN.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            AUTH_CORE_GOLDEN_PIN.split_whitespace().next(),
            Some(digest.as_str())
        );
        assert_eq!(
            minted_user_claims("user_in_standard_org", "model-gateway")["zdr"],
            serde_json::json!(false)
        );
        assert_eq!(
            minted_user_claims("user_in_zdr_org", "model-gateway")["zdr"],
            serde_json::json!(true)
        );
    }

    #[tokio::test]
    #[serial]
    async fn delegated_user_bearers_refuse_a_different_retention_posture() {
        let kid = "test-kid-zdr-posture-mismatch";
        let _server = configure_real_verification(kid).await;
        // Same user and tenant on both sides, so only the posture differs.
        for ingress_zdr in [false, true] {
            let ingress = sign_test_jwt(
                &with_zdr(
                    minted_user_claims("user_in_standard_org", "model-gateway"),
                    ingress_zdr,
                ),
                kid,
            );
            for (header, audience) in DELEGATED_USER_BEARERS {
                let delegate = sign_test_jwt(
                    &with_zdr(
                        minted_user_claims("user_in_standard_org", audience),
                        !ingress_zdr,
                    ),
                    kid,
                );
                let (status, body) = send_with_headers(&[
                    ("authorization", format!("Bearer {ingress}")),
                    (header, format!("Bearer {delegate}")),
                ])
                .await;
                assert_eq!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{header} must be refused when the ingress zdr is {ingress_zdr}"
                );
                assert_eq!(
                    body,
                    mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH.json_body(),
                    "{header}"
                );
            }
        }
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn delegated_user_bearers_with_the_ingress_posture_are_accepted() {
        let kid = "test-kid-zdr-posture-equal";
        let _server = configure_real_verification(kid).await;
        for zdr in [false, true] {
            let ingress = sign_test_jwt(
                &with_zdr(
                    minted_user_claims("user_in_standard_org", "model-gateway"),
                    zdr,
                ),
                kid,
            );
            let mut headers = vec![("authorization", format!("Bearer {ingress}"))];
            for (header, audience) in DELEGATED_USER_BEARERS {
                let delegate = sign_test_jwt(
                    &with_zdr(minted_user_claims("user_in_standard_org", audience), zdr),
                    kid,
                );
                headers.push((header, format!("Bearer {delegate}")));
            }
            let (status, body) = send_with_headers(&headers).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "all ten user bearers at zdr={zdr}: {body}"
            );
        }
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn service_inference_bearer_may_hold_a_different_posture_by_audience() {
        let kid = "test-kid-service-posture-by-audience";
        let _server = configure_real_verification(kid).await;
        // service_claims() is issued with the "auth-core" issuer, not the golden's.
        std::env::set_var("AUTH_CORE_ISSUER", "auth-core");
        // A service principal's retention is per audience in the registry: one
        // real row holds zdr for control-policy and persistent for inference-core
        // (apps/Control Plane/config/plane-service-principals.json). The service
        // variant must not compare posture, so zdr on the gateway's audience and
        // persistent on inference-core is a legitimate pair.
        let ingress = sign_test_jwt(&service_claims(), kid);
        let mut inference = service_claims();
        inference["aud"] = serde_json::json!("inference-core");
        inference["zdr"] = serde_json::json!(false);
        // Inference Core's own scope: the delegate is checked against `inference:invoke`, not the gateway's.
        inference["scopes"] = serde_json::json!(["inference:invoke"]);
        let (status, body) = send_with_headers(&[
            ("authorization", format!("Bearer {ingress}")),
            (
                "x-inference-authorization",
                format!("Bearer {}", sign_test_jwt(&inference, kid)),
            ),
        ])
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn dev_bypass_refuses_a_zdr_delegate_it_cannot_match() {
        let kid = "test-kid-bypass-posture";
        let _server = configure_real_verification(kid).await;
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");
        // The bypass asserts no retention posture of its own; it is the
        // persistent posture, so a ZDR delegate for the same identity is refused.
        let headers = |zdr: bool| {
            let session = sign_test_jwt(
                &with_zdr(
                    minted_user_claims("user_in_standard_org", "session-core"),
                    zdr,
                ),
                kid,
            );
            vec![
                ("authorization", "Bearer dev".to_owned()),
                ("x-org-id", "org-standard-golden".to_owned()),
                ("x-user-id", "user-golden".to_owned()),
                ("x-session-authorization", format!("Bearer {session}")),
            ]
        };
        let (status, body) = send_with_headers(&headers(true)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(
            body,
            mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH.json_body()
        );
        let (status, body) = send_with_headers(&headers(false)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        clear_env();
    }
}

/// WS16.S1: the service route table as a golden-pinned artefact. The table and the second-bearer
/// rule are what a caller's fake gateway reads, so a route or scope added to
/// [`SERVICE_ROUTE_GRANTS`] must change the golden in the same change.
#[cfg(test)]
mod service_route_table_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use axum::{body::Body, http::Request as HttpRequest, middleware, Router};
    use tower::ServiceExt;

    const GOLDEN_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/contracts/model-gateway-service-routes.json"
    );

    fn render() -> String {
        let mut text = serde_json::to_string_pretty(&service_route_table_json())
            .expect("the route table serializes");
        text.push('\n');
        text
    }

    fn pin_line(text: &str) -> String {
        format!(
            "{:x} *model-gateway-service-routes.json\n",
            Sha256::digest(text.as_bytes())
        )
    }

    /// Regenerate with `MP_UPDATE_GOLDEN=1 cargo test -p model-gateway --lib
    /// service_route_table_matches_the_golden`; then update the digest in
    /// `scripts/tests/zdr-contract-sync.test.sh` and re-vendor Quarry's copy.
    #[test]
    fn service_route_table_matches_the_golden() {
        let generated = render();
        let pin_path = format!("{GOLDEN_PATH}.sha256");
        if std::env::var("MP_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
            std::fs::create_dir_all(std::path::Path::new(GOLDEN_PATH).parent().unwrap())
                .expect("the contracts directory");
            std::fs::write(GOLDEN_PATH, &generated).expect("write the route table golden");
            std::fs::write(&pin_path, pin_line(&generated)).expect("write the route table pin");
            return;
        }
        let committed = std::fs::read_to_string(GOLDEN_PATH).unwrap_or_else(|error| {
            panic!(
                "cannot read {GOLDEN_PATH} ({error}). Generate it with MP_UPDATE_GOLDEN=1 cargo test \
                 -p model-gateway --lib service_route_table_matches_the_golden"
            )
        });
        assert!(
            !committed.contains('\r'),
            "the route table golden must keep LF line endings (it is hashed byte for byte)"
        );
        assert_eq!(
            committed, generated,
            "SERVICE_ROUTE_GRANTS differs from the committed golden. If the change is intended, \
             regenerate with MP_UPDATE_GOLDEN=1, refresh the pin and Quarry's vendored copy, and \
             update the digest in scripts/tests/zdr-contract-sync.test.sh."
        );
        let pin = std::fs::read_to_string(&pin_path).expect("the golden's .sha256 pin");
        assert_eq!(pin, pin_line(&committed), "the .sha256 pin differs from the golden");
    }

    fn service(sub: &str, scopes: &[&str]) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: sub.to_owned(),
            iss: "test".to_owned(),
            exp: i64::MAX,
            org_id: "org".to_owned(),
            user_id: String::new(),
            nbf: None,
            aud: None,
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            zdr: false,
            org_zdr: None,
            zdr_waiver: None,
            sovereign: None,
            principal_type: Some("service".to_owned()),
            service_id: Some(sub.to_owned()),
            reason: Some("a reason".to_owned()),
        }
    }

    /// The table keeps the semantics of the `match` it replaced: any service holding
    /// `models:invoke` reaches the two AI routes; the two internal routes are one named service
    /// each; nothing else (notably `/v1/invoke`) is open to a service.
    #[test]
    fn the_table_keeps_the_semantics_of_the_match_it_replaced() {
        let any = service("service:anything", &["models:invoke"]);
        assert!(service_route_allowed(&any, "POST", "/v1/ai/chat"));
        assert!(service_route_allowed(&any, "POST", "/v1/ai/embeddings"));
        assert!(!service_route_allowed(&any, "GET", "/v1/ai/chat"));
        assert!(!service_route_allowed(&any, "POST", "/v1/invoke"));
        assert!(!service_route_allowed(&any, "POST", "/v1/invoke/stream"));
        let no_scope = service("service:anything", &[]);
        assert!(!service_route_allowed(&no_scope, "POST", "/v1/ai/chat"));

        let deletion = service("service:control-space-deletion", &["model:space-delete"]);
        assert!(service_route_allowed(&deletion, "POST", "/v1/internal/space-deletion/threads"));
        let imposter = service("service:control-space-deletion-copy", &["model:space-delete"]);
        assert!(!service_route_allowed(&imposter, "POST", "/v1/internal/space-deletion/threads"));
        assert!(!service_route_allowed(&deletion, "POST", "/v1/ai/chat"));

        let mail = service("service:verevon-gateway", &["mail-task:prepare-thread"]);
        assert!(service_route_allowed(&mail, "POST", "/v1/internal/mail-task/prepare-thread"));
        let wrong = service("service:someone-else", &["mail-task:prepare-thread"]);
        assert!(!service_route_allowed(&wrong, "POST", "/v1/internal/mail-task/prepare-thread"));
    }

    #[test]
    fn the_golden_records_the_second_bearer_and_never_opens_invoke() {
        let table = service_route_table_json();
        assert_eq!(table["service_inference_bearer"]["header"], "x-inference-authorization");
        assert_eq!(table["service_inference_bearer"]["audience"], "inference-core");
        assert_eq!(
            table["service_inference_bearer"]["downstream_requires_scope"],
            "inference:invoke"
        );
        let paths: Vec<&str> = table["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["path"].as_str().unwrap())
            .collect();
        assert!(!paths.iter().any(|path| path.starts_with("/v1/invoke")), "{paths:?}");
    }

    #[test]
    fn service_principal_id_reads_only_a_service_claim() {
        let svc = service("service:quarry-edge", &["models:invoke"]);
        assert_eq!(svc.service_principal_id(), Some("service:quarry-edge"));
        let mut user = svc.clone();
        user.principal_type = Some("user".to_owned());
        assert_eq!(user.service_principal_id(), None);
    }

    /// E3.6-8: the two extra routes are in the table, flagged, and nothing else is.
    #[test]
    fn the_layout_and_pii_rows_carry_their_switch_and_no_other_row_does() {
        let flagged: Vec<(&str, &str)> = SERVICE_ROUTE_GRANTS
            .iter()
            .filter_map(|grant| grant.flag.map(|flag| (grant.path, flag)))
            .collect();
        assert_eq!(
            flagged,
            vec![
                ("/v1/ai/documents/layout", "MODEL_GATEWAY_AI_LAYOUT_SERVICE"),
                ("/v1/ai/language/pii", "MODEL_GATEWAY_AI_PII_SERVICE"),
            ]
        );
        for grant in SERVICE_ROUTE_GRANTS.iter().filter(|g| g.flag.is_some()) {
            assert_eq!(grant.method, "POST");
            assert_eq!(grant.scope, "models:invoke");
            assert_eq!(grant.service, None);
        }
        // Still never /v1/invoke, and no sibling route of the flagged two opened by accident.
        let any = service("service:anything", &["models:invoke"]);
        assert!(service_route_allowed(&any, "POST", "/v1/ai/documents/layout"));
        assert!(service_route_allowed(&any, "POST", "/v1/ai/language/pii"));
        for path in [
            "/v1/ai/documents",
            "/v1/ai/documents/analyze",
            "/v1/ai/documents/forms",
            "/v1/ai/language",
            "/v1/ai/language/entities",
            "/v1/ai/language/detect",
        ] {
            assert!(!service_route_allowed(&any, "POST", path), "{path}");
        }
        assert!(!service_route_allowed(&any, "GET", "/v1/ai/language/pii"));
    }

    #[test]
    fn a_service_switch_is_on_only_for_the_three_literals() {
        for on in ["1", "true", "on", " 1 "] {
            assert!(service_flag_on(Some(on)), "{on:?}");
        }
        for off in ["", "0", "false", "off", "yes", "TRUE", "enabled", "2"] {
            assert!(!service_flag_on(Some(off)), "{off:?}");
        }
        assert!(!service_flag_on(None));
    }

    #[test]
    fn a_flagged_route_is_switched_off_by_default_and_only_for_a_caller_who_holds_it() {
        let svc = service("service:quarry-edge", &["models:invoke"]);
        let unset = |_: &str| None;
        let only = |name: &'static str| {
            move |asked: &str| (asked == name).then(|| "1".to_owned())
        };
        for path in ["/v1/ai/language/pii", "/v1/ai/documents/layout"] {
            assert!(service_route_switched_off(&svc, "POST", path, &unset), "{path}");
        }
        // Each switch opens its own route only.
        let pii_on = only("MODEL_GATEWAY_AI_PII_SERVICE");
        assert!(!service_route_switched_off(&svc, "POST", "/v1/ai/language/pii", &pii_on));
        assert!(service_route_switched_off(&svc, "POST", "/v1/ai/documents/layout", &pii_on));
        let layout_on = only("MODEL_GATEWAY_AI_LAYOUT_SERVICE");
        assert!(!service_route_switched_off(&svc, "POST", "/v1/ai/documents/layout", &layout_on));
        assert!(service_route_switched_off(&svc, "POST", "/v1/ai/language/pii", &layout_on));
        // A switch value that is not a literal stays off.
        let wrong = |_: &str| Some("yes".to_owned());
        assert!(service_route_switched_off(&svc, "POST", "/v1/ai/language/pii", &wrong));
        // No scope, wrong method, an unflagged route: not this function's business (403 / allowed).
        let no_scope = service("service:quarry-edge", &[]);
        assert!(!service_route_switched_off(&no_scope, "POST", "/v1/ai/language/pii", &unset));
        assert!(!service_route_switched_off(&svc, "GET", "/v1/ai/language/pii", &unset));
        assert!(!service_route_switched_off(&svc, "POST", "/v1/ai/chat", &unset));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn the_middleware_answers_404_while_the_switch_is_off_and_403_without_the_scope() {
        async fn ok_handler() -> &'static str {
            "ok"
        }
        async fn status_of(claims: Claims, path: &str) -> StatusCode {
            Router::new()
                .route("/v1/ai/language/pii", axum::routing::post(ok_handler))
                .route("/v1/ai/documents/layout", axum::routing::post(ok_handler))
                .route("/v1/ai/chat", axum::routing::post(ok_handler))
                .layer(middleware::from_fn(authorize_principal_route))
                .layer(axum::Extension(claims))
                .oneshot(
                    HttpRequest::builder()
                        .method("POST")
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
        }
        std::env::remove_var(SERVICE_FLAG_AI_PII);
        std::env::remove_var(SERVICE_FLAG_AI_LAYOUT);
        let svc = service("service:quarry-edge", &["models:invoke"]);
        let no_scope = service("service:quarry-edge", &[]);

        assert_eq!(status_of(svc.clone(), "/v1/ai/language/pii").await, StatusCode::NOT_FOUND);
        assert_eq!(status_of(svc.clone(), "/v1/ai/documents/layout").await, StatusCode::NOT_FOUND);
        assert_eq!(status_of(svc.clone(), "/v1/ai/chat").await, StatusCode::OK);
        assert_eq!(status_of(no_scope, "/v1/ai/language/pii").await, StatusCode::FORBIDDEN);

        std::env::set_var(SERVICE_FLAG_AI_PII, "1");
        assert_eq!(status_of(svc.clone(), "/v1/ai/language/pii").await, StatusCode::OK);
        assert_eq!(status_of(svc.clone(), "/v1/ai/documents/layout").await, StatusCode::NOT_FOUND);
        std::env::set_var(SERVICE_FLAG_AI_LAYOUT, "true");
        assert_eq!(status_of(svc.clone(), "/v1/ai/documents/layout").await, StatusCode::OK);
        std::env::set_var(SERVICE_FLAG_AI_PII, "0");
        assert_eq!(status_of(svc, "/v1/ai/language/pii").await, StatusCode::NOT_FOUND);
        std::env::remove_var(SERVICE_FLAG_AI_PII);
        std::env::remove_var(SERVICE_FLAG_AI_LAYOUT);
    }
}
