//! Auth Core JWT verification for the Execution Core gRPC boundary.
//!
//! Execution Core accepts only a short-lived, user-bound Execution Core bearer
//! as local authority. Credentials for Data Plane, Session Core, and Inference
//! Core are delegated separately, independently verified, identity-bound, and
//! used only for their intended downstream service.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Resolution, RouteView, WaiverView, WAIVER_CONTRACT_VERSION,
};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tonic::{Request, Status};
use tracing::{error, warn};

const DEFAULT_JWKS_TTL_SECS: u64 = 300;
const DEFAULT_JWT_LEEWAY_SECS: u64 = 30;
const MAX_JWT_LEEWAY_SECS: u64 = 300;
const MAX_JWKS_BYTES: usize = 1_048_576;

#[derive(Clone)]
pub struct JwtVerifier {
    issuer: Arc<str>,
    audience: Arc<str>,
    jwks_url: Arc<str>,
    jwks_ttl: Duration,
    leeway_secs: u64,
    client: reqwest::Client,
    cache: Arc<RwLock<CachedJwks>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl fmt::Debug for JwtVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JwtVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("jwks_url", &self.jwks_url)
            .field("jwks_ttl", &self.jwks_ttl)
            .field("leeway_secs", &self.leeway_secs)
            .finish_non_exhaustive()
    }
}

struct CachedJwks {
    fetched_at: Instant,
    jwks: JwkSet,
    revision: u64,
    missing_kids: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[allow(dead_code)]
    iss: String,
    #[allow(dead_code)]
    aud: serde_json::Value,
    #[allow(dead_code)]
    exp: u64,
    #[serde(default)]
    #[allow(dead_code)]
    nbf: Option<u64>,
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    principal_type: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    zdr: bool,
    /// `org_zdr`: `true` when the caller's org is Zero Data Retention. Absent on
    /// service tokens. Only a waiver reads it, and only as `Some(true)`.
    #[serde(default)]
    org_zdr: Option<bool>,
    /// `zdr_waiver`: present only on a persistent user token of a ZDR org, bound
    /// to one connection. Parsed and checked by [`parse_zdr_waiver`].
    #[serde(default)]
    zdr_waiver: Option<serde_json::Value>,
}

/// The connection-bound waiver a persistent user token of a ZDR org carries (Z-18).
///
/// Auth Core stamps it only when the org is ZDR and one user-owned connection that
/// cannot give zero retention was acknowledged for the turn. It is read from the
/// signed `zdr_waiver` claim, parsed and checked by the shared resolver
/// (`mp_contracts::retention`), and is never built from a request field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZdrWaiver {
    version: u32,
    provider: String,
    connection_id: String,
    ack_id: String,
}

impl ZdrWaiver {
    /// The waiver contract version the token was minted under.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The canonical provider the waiver binds.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The one user connection the waiver binds.
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// The acknowledgement the waiver rests on.
    #[must_use]
    pub fn ack_id(&self) -> &str {
        &self.ack_id
    }
}

/// Verified caller identity. The Execution Core ingress credential is never
/// retained or reused as authority at another service boundary.
#[derive(Clone)]
pub struct AuthenticatedUser {
    pub org_id: String,
    pub user_id: String,
    pub zdr: bool,
    /// The `org_zdr` claim: `Some(true)` when the caller's org is Zero Data
    /// Retention, `None` when the token does not say (Z-18).
    pub org_zdr: Option<bool>,
    /// The waiver the token carries, when it carries one (Z-18). A run's route
    /// and every delegated bearer are judged against it.
    pub zdr_waiver: Option<ZdrWaiver>,
}

/// Verified service identity for the dedicated scheduled-step ingress.
///
/// This is intentionally a separate type from [`AuthenticatedUser`]. A
/// service credential can authorize only the explicitly allowlisted
/// service-owned RPC; it is never accepted by the user-bound ExecuteStep or
/// RunAgent paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedService {
    pub org_id: String,
    pub service_id: String,
    scopes: Arc<[String]>,
    pub zdr: bool,
}

impl AuthenticatedService {
    #[cfg(test)]
    pub(crate) fn for_test(org_id: &str, service_id: &str, scopes: &[&str], zdr: bool) -> Self {
        Self {
            org_id: org_id.to_owned(),
            service_id: service_id.to_owned(),
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            zdr,
        }
    }

    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|candidate| candidate == scope)
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize_org(&self, org_id: &str) -> Result<(), Status> {
        if org_id.trim().is_empty() || org_id != self.org_id {
            return Err(Status::permission_denied("service tenant access denied"));
        }
        Ok(())
    }
}

/// Opaque, independently verified `aud=data-plane` credential. It is bound to
/// the same canonical user and tenant as the Execution Core ingress token and
/// is the only user credential permitted on Data Plane calls.
#[derive(Clone)]
pub struct DelegatedDataPlaneBearer(Arc<str>);

impl DelegatedDataPlaneBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(Arc::from("data-plane-test-bearer"))
    }
}

/// Opaque delegated session-core credential. Execution Core does not treat it
/// as local authority: `bind_session_bearer` checks its audience and that its
/// retention posture is the caller's, then it is forwarded to session-core, which
/// independently verifies `aud=session-core` and pins resource ownership. Debug
/// is redacted.
#[derive(Clone)]
pub struct DelegatedSessionBearer(Arc<str>);

impl DelegatedSessionBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(Arc::from("session-test-bearer"))
    }
}

/// Opaque, independently verified `aud=inference-core` credential. It is bound
/// to the same canonical user and tenant as the Execution Core ingress token.
#[derive(Clone)]
pub struct DelegatedInferenceBearer(Arc<str>);

impl DelegatedInferenceBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque, independently verified `aud=browser-broker` credential. It is
/// bound to the same canonical user, tenant, and ZDR posture as the Execution
/// Core caller and is used only to revalidate a browser grant before dispatch.
#[derive(Clone)]
pub struct DelegatedBrowserBearer(Arc<str>);

impl DelegatedBrowserBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque, independently verified `aud=sandbox-manager` credential. It is
/// bound to the same canonical user, tenant, and ZDR posture as the Execution
/// Core caller and is presented to sandbox-manager's `AcquireLease` ONLY —
/// that RPC's own identity check binds a Space capability decision's subject
/// to the CALLING principal, which a service-level credential can never
/// satisfy for an actual Space member. See
/// `apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md`
/// §3.5 phase B.2.
#[derive(Clone)]
pub struct DelegatedSandboxBearer(Arc<str>);

impl DelegatedSandboxBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DelegatedInferenceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedInferenceBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedBrowserBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedBrowserBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedSandboxBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedSandboxBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedDataPlaneBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedDataPlaneBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedSessionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedSessionBearer([REDACTED])")
    }
}

/// Extract the independently delegated Session Core bearer.
///
/// # Errors
/// Returns `unauthenticated` when the metadata is missing or malformed.
#[allow(clippy::result_large_err)]
pub fn delegated_session_bearer<T>(request: &Request<T>) -> Result<DelegatedSessionBearer, Status> {
    request
        .metadata()
        .get("x-session-authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .map(|value| DelegatedSessionBearer(Arc::from(value)))
        .ok_or_else(|| Status::unauthenticated("delegated session credential required"))
}

impl fmt::Debug for AuthenticatedUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedUser")
            .field("org_id", &self.org_id)
            .field("user_id", &self.user_id)
            .field("zdr", &self.zdr)
            .field("org_zdr", &self.org_zdr)
            .field("zdr_waiver", &self.zdr_waiver.is_some())
            .finish()
    }
}

impl AuthenticatedUser {
    #[cfg(test)]
    pub(crate) fn for_test(org_id: &str, user_id: &str) -> Self {
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            zdr: false,
            org_zdr: None,
            zdr_waiver: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_zdr(org_id: &str, user_id: &str, zdr: bool) -> Self {
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            zdr,
            org_zdr: None,
            zdr_waiver: None,
        }
    }

    /// A persistent caller of a ZDR org, bound to one connection by a waiver
    /// (Z-18). The waiver is the one Auth Core stamps for that connection.
    #[cfg(test)]
    pub(crate) fn for_test_with_waiver(
        org_id: &str,
        user_id: &str,
        connection_id: &str,
        ack_id: &str,
    ) -> Self {
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            zdr: false,
            org_zdr: Some(true),
            zdr_waiver: Some(ZdrWaiver {
                version: mp_contracts::retention::WAIVER_CONTRACT_VERSION,
                provider: mp_contracts::agentic_subscription::PROVIDER_ID.to_owned(),
                connection_id: connection_id.to_owned(),
                ack_id: ack_id.to_owned(),
            }),
        }
    }

    /// This caller's posture as the shared retention resolver reads it (Z-18).
    #[must_use]
    pub(crate) fn retention_claims(&self) -> mp_contracts::retention::ClaimsView<'_> {
        mp_contracts::retention::ClaimsView {
            zdr: self.zdr,
            org_zdr: self.org_zdr,
            waiver: self
                .zdr_waiver
                .as_ref()
                .map(|waiver| mp_contracts::retention::WaiverView {
                    version: waiver.version,
                    provider: &waiver.provider,
                    connection: &waiver.connection_id,
                }),
        }
    }

    /// Bind tenant and, when supplied by the RPC, user identity to the signed
    /// claims. Empty request identities are rejected rather than treated as a
    /// wildcard.
    ///
    /// # Errors
    /// Returns `permission_denied` when the request attempts to select another
    /// tenant or user and `invalid_argument` when identity is absent.
    #[allow(clippy::result_large_err)]
    pub fn authorize(&self, org_id: &str, user_id: Option<&str>) -> Result<(), Status> {
        if org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if org_id != self.org_id {
            warn!("execution request tenant does not match verified credential");
            return Err(Status::permission_denied("tenant access denied"));
        }
        if let Some(user_id) = user_id {
            if user_id.trim().is_empty() {
                return Err(Status::invalid_argument("user_id is required"));
            }
            if user_id != self.user_id {
                warn!("execution request user does not match verified credential");
                return Err(Status::permission_denied("user access denied"));
            }
        }
        Ok(())
    }
}

pub(crate) struct AuthConfig {
    pub(crate) jwks_url: String,
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) jwks_ttl: Duration,
    pub(crate) leeway_secs: u64,
}

impl AuthConfig {
    fn from_env() -> anyhow::Result<Self> {
        let leeway_secs = optional_u64("AUTH_CORE_JWT_LEEWAY_SECS", DEFAULT_JWT_LEEWAY_SECS)
            .min(MAX_JWT_LEEWAY_SECS);
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("EXECUTION_CORE_AUTH_AUDIENCE")?,
            jwks_ttl: Duration::from_secs(optional_u64(
                "AUTH_CORE_JWKS_TTL_SECS",
                DEFAULT_JWKS_TTL_SECS,
            )),
            leeway_secs,
        })
    }
}

fn required_env(name: &'static str) -> anyhow::Result<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} is required"))
}

fn optional_u64(name: &'static str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

impl JwtVerifier {
    /// Load authentication configuration and fetch the initial JWKS before the
    /// gRPC listener becomes ready. Missing or unavailable verification
    /// material therefore fails startup closed.
    ///
    /// # Errors
    /// Returns an error for missing configuration, an invalid HTTP client, or
    /// an unavailable/malformed JWKS response.
    pub async fn from_env() -> anyhow::Result<Self> {
        Self::from_config(AuthConfig::from_env()?).await
    }

    pub(crate) async fn from_config(config: AuthConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()?;
        let jwks = fetch_jwks(&client, &config.jwks_url).await?;
        Ok(Self {
            issuer: Arc::from(config.issuer),
            audience: Arc::from(config.audience),
            jwks_url: Arc::from(config.jwks_url),
            jwks_ttl: config.jwks_ttl,
            leeway_secs: config.leeway_secs,
            client,
            cache: Arc::new(RwLock::new(CachedJwks {
                fetched_at: Instant::now(),
                jwks,
                revision: 0,
                missing_kids: HashSet::new(),
            })),
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Verify the gRPC bearer cryptographically and return its canonical user
    /// identity. Only RS256 Auth Core user tokens are accepted.
    ///
    /// # Errors
    /// Returns `unauthenticated` for malformed, forged, expired, wrong-issuer,
    /// wrong-audience, or ambiguous credentials. JWKS refresh outages return
    /// `unavailable` and remain fail-closed.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate<T>(&self, request: &Request<T>) -> Result<AuthenticatedUser, Status> {
        let token = extract_bearer(request)?;
        self.verify_user_token(token, &self.audience).await
    }

    /// Verify the service credential used only by the scheduled-step lane.
    /// The caller is still checked for the exact service principal and scope
    /// by the RPC handler; this method only establishes the signed identity.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_scheduled_step<T>(
        &self,
        request: &Request<T>,
    ) -> Result<AuthenticatedService, Status> {
        let token = extract_bearer(request)?;
        self.verify_service_token(token, &self.audience).await
    }

    /// Verify a separately delegated inference credential and bind it to the
    /// already authenticated Execution Core caller. Missing, malformed, or
    /// wrong-audience credentials fail closed before a run is dispatched.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for an invalid,
    /// unavailable, wrong-audience, or identity-mismatched credential.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_inference<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedInferenceBearer, Status> {
        let token = extract_metadata_bearer(request, "x-inference-authorization")?;
        let delegated = self.verify_user_token(token, "inference-core").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated inference identity does not match caller",
            ));
        }
        if delegated.zdr != caller.zdr || delegated.zdr_waiver != caller.zdr_waiver {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(DelegatedInferenceBearer(Arc::from(token)))
    }

    /// Verify a separately delegated BrowserBroker credential and bind it to
    /// the authenticated execution caller. A browser grant is revalidated with
    /// this token immediately before any Quarry launch, so a model-authored
    /// `grant_id` or forged scoping header can never provide authority.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for a missing,
    /// malformed, wrong-audience, unavailable, or identity-mismatched bearer.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_browser<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedBrowserBearer, Status> {
        let token = extract_metadata_bearer(request, "x-browser-authorization")?;
        let delegated = self.verify_user_token(token, "browser-broker").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated browser identity does not match caller",
            ));
        }
        if delegated.zdr != caller.zdr || delegated.zdr_waiver != caller.zdr_waiver {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(DelegatedBrowserBearer(Arc::from(token)))
    }

    /// Verify the run's browser bearer when the caller presents one (WS21-S2). An absent header is
    /// `None`, not an error: a run without browser authority is an ordinary run. A present one is
    /// checked exactly as [`Self::authenticate_delegated_browser`] checks it; whether the Do loop
    /// may use it is decided by the run's offer.
    ///
    /// # Errors
    ///
    /// Returns the statuses of [`Self::authenticate_delegated_browser`] for a malformed,
    /// wrong-audience, unavailable or identity-mismatched bearer.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_run_browser<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<Option<DelegatedBrowserBearer>, Status> {
        if !request.metadata().contains_key("x-browser-authorization") {
            return Ok(None);
        }
        self.authenticate_delegated_browser(request, caller)
            .await
            .map(Some)
    }

    /// Verify a separately delegated Data Plane credential and bind it to the
    /// already authenticated Execution Core caller. The ingress token is never
    /// reused for retrieval, including when its syntax is otherwise valid.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for an invalid,
    /// unavailable, wrong-audience, or identity-mismatched credential.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_data_plane<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedDataPlaneBearer, Status> {
        let token = extract_metadata_bearer(request, "x-data-plane-authorization")?;
        let delegated = self.verify_user_token(token, "data-plane").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated Data Plane identity does not match caller",
            ));
        }
        if delegated.zdr != caller.zdr || delegated.zdr_waiver != caller.zdr_waiver {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(DelegatedDataPlaneBearer(Arc::from(token)))
    }

    /// Verify a separately delegated sandbox-manager credential and bind it to
    /// the already authenticated Execution Core caller. Demanded only for a
    /// Space-scoped `code_interpreter` step (see `grpc.rs::execute_step`) —
    /// every other step never calls this, so an older or non-Space caller is
    /// unaffected.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for a missing,
    /// invalid, unavailable, wrong-audience, or identity-mismatched
    /// credential.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_sandbox_manager<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedSandboxBearer, Status> {
        let token = extract_metadata_bearer(request, "x-sandbox-authorization")?;
        let delegated = self.verify_user_token(token, "sandbox-manager").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated sandbox-manager identity does not match caller",
            ));
        }
        if delegated.zdr != caller.zdr || delegated.zdr_waiver != caller.zdr_waiver {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(DelegatedSandboxBearer(Arc::from(token)))
    }

    /// Bind the session-core bearer of a run to its caller (Z-18). Every user
    /// bearer of one turn carries the same retention posture (matrix rule 1), and
    /// the other delegated bearers are compared with the caller in this module.
    /// Session-core judges its own bearer only, so without this check a run could
    /// be recorded under one posture and served under another.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for a missing, invalid,
    /// wrong-audience or identity-mismatched bearer, or one whose retention posture
    /// or waiver differs from the caller's.
    #[allow(clippy::result_large_err)]
    pub async fn bind_session_bearer(
        &self,
        bearer: &DelegatedSessionBearer,
        caller: &AuthenticatedUser,
    ) -> Result<(), Status> {
        let token = bearer.as_str();
        let delegated = self.verify_user_token(token, "session-core").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated session identity does not match caller",
            ));
        }
        if delegated.zdr != caller.zdr || delegated.zdr_waiver != caller.zdr_waiver {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(())
    }

    async fn verify_user_token(
        &self,
        token: &str,
        audience: &str,
    ) -> Result<AuthenticatedUser, Status> {
        let header =
            decode_header(token).map_err(|_| Status::unauthenticated("invalid user credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid user credential"));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.trim().is_empty())
            .ok_or_else(|| Status::unauthenticated("invalid user credential"))?;
        let jwk = self.jwk_for(&kid).await?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|error| {
            error!(%error, "Auth Core JWKS contains an unusable signing key");
            Status::unavailable("authentication verification unavailable")
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| Status::unauthenticated("invalid user credential"))?
            .claims;
        validate_user_claims(claims)
    }

    async fn verify_service_token(
        &self,
        token: &str,
        audience: &str,
    ) -> Result<AuthenticatedService, Status> {
        let header = decode_header(token)
            .map_err(|_| Status::unauthenticated("invalid service credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid service credential"));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.trim().is_empty())
            .ok_or_else(|| Status::unauthenticated("invalid service credential"))?;
        let jwk = self.jwk_for(&kid).await?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|error| {
            error!(%error, "Auth Core JWKS contains an unusable signing key");
            Status::unavailable("authentication verification unavailable")
        })?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| Status::unauthenticated("invalid service credential"))?
            .claims;
        validate_service_claims(claims)
    }

    async fn jwk_for(&self, kid: &str) -> Result<jsonwebtoken::jwk::Jwk, Status> {
        let observed_revision = {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid user credential"));
                }
            }
            cache.revision
        };

        // Coalesce refreshes. Re-check after acquiring the lock so another
        // request's successful refresh is reused.
        let _refresh = self.refresh_lock.lock().await;
        {
            let cache = self.cache.read().await;
            if cache.revision != observed_revision {
                return cache
                    .jwks
                    .find(kid)
                    .cloned()
                    .ok_or_else(|| Status::unauthenticated("invalid user credential"));
            }
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid user credential"));
                }
            }
        }
        let jwks = fetch_jwks(&self.client, &self.jwks_url)
            .await
            .map_err(|error| {
                error!(%error, "failed to refresh Auth Core JWKS");
                Status::unavailable("authentication verification unavailable")
            })?;
        let jwk = jwks.find(kid).cloned();
        let mut cache = self.cache.write().await;
        let revision = cache.revision.saturating_add(1);
        let missing_kids = if jwk.is_none() {
            HashSet::from([kid.to_owned()])
        } else {
            HashSet::new()
        };
        *cache = CachedJwks {
            fetched_at: Instant::now(),
            jwks,
            revision,
            missing_kids,
        };
        jwk.ok_or_else(|| Status::unauthenticated("invalid user credential"))
    }
}

#[allow(clippy::result_large_err)]
fn extract_bearer<T>(request: &Request<T>) -> Result<&str, Status> {
    extract_metadata_bearer(request, "authorization")
}

#[allow(clippy::result_large_err)]
fn extract_metadata_bearer<'a, T>(
    request: &'a Request<T>,
    header: &'static str,
) -> Result<&'a str, Status> {
    request
        .metadata()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified delegated credential required"))
}

/// The wire shape of the `zdr_waiver` claim. Extra fields Auth Core may add (a
/// digest, a decision id) are ignored; these four are required.
#[derive(Debug, Deserialize)]
struct WaiverClaim {
    version: u32,
    provider: String,
    connection: String,
    ack_id: String,
}

/// Parse and check the `zdr_waiver` claim of one verified user token (Z-18).
///
/// Absent or `null` is no waiver. Anything else must be a waiver the shared
/// resolver accepts: a persistent token (`zdr` false) of a ZDR org (`org_zdr`
/// true), bound to one provider and one connection, at the current contract
/// version, with a non-empty acknowledgement id. A malformed claim, an unknown
/// version or a waiver on a ZDR token is refused, never read as "no waiver": a
/// token that silently dropped its waiver would be served as an ordinary turn of
/// a ZDR org. session-core's `parse_zdr_waiver` judges the same claim the same way.
#[allow(clippy::result_large_err)]
fn parse_zdr_waiver(
    zdr: bool,
    org_zdr: Option<bool>,
    claim: Option<&serde_json::Value>,
) -> Result<Option<ZdrWaiver>, Status> {
    let refused = || Status::unauthenticated("invalid user credential");
    let Some(claim) = claim.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let waiver: WaiverClaim = serde_json::from_value(claim.clone()).map_err(|_| refused())?;
    if zdr || org_zdr != Some(true) {
        return Err(refused());
    }
    // The shared resolver is the one definition of a valid waiver: resolving the
    // waiver's own route must yield a waiver for exactly that connection.
    let resolution = resolve_turn(
        &[ClaimsView {
            zdr: false,
            org_zdr: Some(true),
            waiver: Some(WaiverView {
                version: waiver.version,
                provider: waiver.provider.as_str(),
                connection: waiver.connection.as_str(),
            }),
        }],
        &RouteView {
            provider: waiver.provider.as_str(),
            connection: Some(waiver.connection.as_str()),
            explicit_zdr: false,
        },
    );
    if !matches!(resolution, Resolution::Waived { .. }) || waiver.version != WAIVER_CONTRACT_VERSION
    {
        return Err(refused());
    }
    for id in [&waiver.connection, &waiver.ack_id] {
        if id.is_empty()
            || id.len() > 128
            || id.trim() != id.as_str()
            || id.chars().any(char::is_control)
        {
            return Err(refused());
        }
    }
    Ok(Some(ZdrWaiver {
        version: waiver.version,
        provider: waiver.provider,
        connection_id: waiver.connection,
        ack_id: waiver.ack_id,
    }))
}

#[allow(clippy::result_large_err)]
fn validate_user_claims(claims: Claims) -> Result<AuthenticatedUser, Status> {
    let is_user = claims.principal_type.as_deref().unwrap_or("user") == "user";
    if !is_user
        || claims.sub.trim().is_empty()
        || claims.user_id.trim().is_empty()
        || claims.org_id.trim().is_empty()
        || claims.sub != claims.user_id
        || claims.user_id.trim() != claims.user_id
        || claims.org_id.trim() != claims.org_id
        || claims.service_id.is_some()
    {
        return Err(Status::unauthenticated("invalid user credential"));
    }
    let zdr_waiver = parse_zdr_waiver(claims.zdr, claims.org_zdr, claims.zdr_waiver.as_ref())?;
    // The matrix's rule 3 (`e_org_zdr_without_waiver_is_not_persistent`): a
    // persistent token of a ZDR org is served only under its waiver.
    if !claims.zdr && claims.org_zdr == Some(true) && zdr_waiver.is_none() {
        return Err(Status::unauthenticated("invalid user credential"));
    }
    Ok(AuthenticatedUser {
        org_id: claims.org_id,
        user_id: claims.user_id,
        zdr: claims.zdr,
        org_zdr: claims.org_zdr,
        zdr_waiver,
    })
}

#[allow(clippy::result_large_err)]
fn validate_service_claims(claims: Claims) -> Result<AuthenticatedService, Status> {
    let service_id = claims.service_id.as_deref().unwrap_or_default();
    if claims.principal_type.as_deref() != Some("service")
        || claims.sub.trim().is_empty()
        || claims.sub != service_id
        || !service_id.starts_with("service:")
        || claims.user_id.trim() != claims.user_id
        || !claims.user_id.is_empty()
        || claims.org_id.trim().is_empty()
        || claims.org_id.trim() != claims.org_id
        || claims.scopes.len() > 64
        || claims
            .scopes
            .iter()
            .any(|scope| scope.is_empty() || scope.len() > 128 || scope.trim() != scope)
        // A waiver is a decision about one human's connection: a service
        // credential never carries one, whatever else it claims (Z-18).
        || claims
            .zdr_waiver
            .as_ref()
            .is_some_and(|waiver| !waiver.is_null())
    {
        return Err(Status::unauthenticated("invalid service credential"));
    }
    Ok(AuthenticatedService {
        org_id: claims.org_id,
        service_id: service_id.to_owned(),
        scopes: claims.scopes.into(),
        zdr: claims.zdr,
    })
}

async fn fetch_jwks(client: &reqwest::Client, url: &str) -> anyhow::Result<JwkSet> {
    let response = client.get(url).send().await?.error_for_status()?;
    let body = response.bytes().await?;
    if body.len() > MAX_JWKS_BYTES {
        anyhow::bail!("Auth Core JWKS response exceeds size limit");
    }
    validate_jwks_document(&body)?;
    let jwks: JwkSet = serde_json::from_slice(&body)?;
    if jwks.keys.is_empty() {
        anyhow::bail!("Auth Core JWKS contains no signing keys");
    }
    Ok(jwks)
}

#[derive(Deserialize)]
struct RawJwkSet {
    keys: Vec<RawJwk>,
}

#[derive(Deserialize)]
struct RawJwk {
    kty: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    alg: Option<String>,
    kid: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

fn validate_jwks_document(body: &[u8]) -> anyhow::Result<()> {
    use std::collections::HashSet;

    let raw: RawJwkSet = serde_json::from_slice(body)?;
    let mut usable = 0_usize;
    let mut kids = HashSet::new();
    for key in raw.keys {
        if key.kty.as_deref() != Some("RSA") || key.alg.as_deref() != Some("RS256") {
            continue;
        }
        if key.key_use.as_deref().is_some_and(|value| value != "sig") {
            anyhow::bail!("Auth Core JWKS RSA key is not a signing key");
        }
        let kid = key
            .kid
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing kid"))?;
        if !kids.insert(kid) {
            anyhow::bail!("Auth Core JWKS contains a duplicate key id");
        }
        let modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
            key.n
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing modulus"))?,
        )?;
        if rsa_modulus_bits(&modulus) < 2048 {
            anyhow::bail!("Auth Core JWKS RSA key is smaller than 2048 bits");
        }
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
            key.e
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing exponent"))?,
        )?;
        if exponent.iter().all(|byte| *byte == 0) {
            anyhow::bail!("Auth Core JWKS RSA exponent is invalid");
        }
        usable = usable.saturating_add(1);
    }
    if usable == 0 {
        anyhow::bail!("Auth Core JWKS contains no usable RS256 signing keys");
    }
    Ok(())
}

fn rsa_modulus_bits(modulus: &[u8]) -> usize {
    let Some((index, first)) = modulus.iter().enumerate().find(|(_, byte)| **byte != 0) else {
        return 0;
    };
    (modulus.len() - index - 1) * 8 + (8 - first.leading_zeros() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use rsa::traits::PublicKeyParts;
    use serde_json::{json, Value};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn rsa_keypair() -> &'static (String, String, String) {
        static KEYS: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
        KEYS.get_or_init(|| {
            let mut rng = rand::thread_rng();
            let private = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
            let public = rsa::RsaPublicKey::from(&private);
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private PEM")
                .to_string();
            let n =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
            let e =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
            (private_pem, n, e)
        })
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_secs()
    }

    fn claims() -> Value {
        json!({
            "sub": "user-1",
            "iss": "auth-core",
            "aud": "execution-core",
            "exp": now() + 3600,
            "nbf": now() - 5,
            "org_id": "org-1",
            "user_id": "user-1",
            "principal_type": "user",
            "zdr": true
        })
    }

    fn sign(claims: &Value, kid: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_owned());
        encode(
            &header,
            claims,
            &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
        )
        .expect("JWT")
    }

    async fn verifier(kid: &str) -> JwtVerifier {
        let server = MockServer::start().await;
        let (_, n, e) = rsa_keypair();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": kid, "n": n, "e": e
                }]
            })))
            .mount(&server)
            .await;
        JwtVerifier::from_config(AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: "execution-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    fn authenticated_request(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    #[test]
    fn delegated_session_credential_is_required_and_redacted() {
        assert_eq!(
            delegated_session_bearer(&Request::new(()))
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-session-authorization",
            "Bearer dedicated-session-token".parse().expect("metadata"),
        );
        let bearer = delegated_session_bearer(&request).expect("delegated bearer");
        assert_eq!(bearer.as_str(), "dedicated-session-token");
        assert_eq!(format!("{bearer:?}"), "DelegatedSessionBearer([REDACTED])");
    }

    #[tokio::test]
    async fn delegated_inference_credential_requires_exact_audience_and_identity() {
        let verifier = verifier("key-inference").await;
        let execution_token = sign(&claims(), "key-inference");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let mut inference_claims = claims();
        inference_claims["aud"] = json!("inference-core");
        let inference_token = sign(&inference_claims, "key-inference");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {inference_token}")
                .parse()
                .expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_inference(&request, &caller)
            .await
            .expect("delegated inference bearer");
        assert_eq!(delegated.as_str(), inference_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedInferenceBearer([REDACTED])"
        );

        let wrong_audience = sign(&claims(), "key-inference");
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {wrong_audience}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_inference(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = inference_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-inference");
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_inference(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn delegated_browser_credential_requires_exact_audience_and_identity() {
        let verifier = verifier("key-browser").await;
        let execution_token = sign(&claims(), "key-browser");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let missing = Request::new(());
        assert_eq!(
            verifier
                .authenticate_delegated_browser(&missing, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut browser_claims = claims();
        browser_claims["aud"] = json!("browser-broker");
        let browser_token = sign(&browser_claims, "key-browser");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {browser_token}").parse().expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_browser(&request, &caller)
            .await
            .expect("delegated browser bearer");
        assert_eq!(delegated.as_str(), browser_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedBrowserBearer([REDACTED])"
        );

        request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {execution_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_browser(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = browser_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-browser");
        request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_browser(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn run_browser_credential_is_optional_and_otherwise_verified_like_the_delegated_one() {
        let verifier = verifier("key-browser").await;
        let execution_token = sign(&claims(), "key-browser");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        // A run that presents no browser bearer is an ordinary run, not a refusal.
        let absent = Request::new(());
        assert!(verifier
            .authenticate_run_browser(&absent, &caller)
            .await
            .expect("an absent browser bearer is not an error")
            .is_none());

        let mut browser_claims = claims();
        browser_claims["aud"] = json!("browser-broker");
        let browser_token = sign(&browser_claims, "key-browser");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {browser_token}").parse().expect("metadata"),
        );
        let delegated = verifier
            .authenticate_run_browser(&request, &caller)
            .await
            .expect("a verified browser bearer")
            .expect("a present bearer is returned");
        assert_eq!(delegated.as_str(), browser_token);

        // A present bearer of another audience is refused, never silently dropped.
        request.metadata_mut().insert(
            "x-browser-authorization",
            format!("Bearer {execution_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_run_browser(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[tokio::test]
    async fn delegated_sandbox_credential_requires_exact_audience_and_identity() {
        let verifier = verifier("key-sandbox").await;
        let execution_token = sign(&claims(), "key-sandbox");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let missing = Request::new(());
        assert_eq!(
            verifier
                .authenticate_delegated_sandbox_manager(&missing, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut sandbox_claims = claims();
        sandbox_claims["aud"] = json!("sandbox-manager");
        let sandbox_token = sign(&sandbox_claims, "key-sandbox");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-sandbox-authorization",
            format!("Bearer {sandbox_token}").parse().expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_sandbox_manager(&request, &caller)
            .await
            .expect("delegated sandbox bearer");
        assert_eq!(delegated.as_str(), sandbox_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedSandboxBearer([REDACTED])"
        );

        request.metadata_mut().insert(
            "x-sandbox-authorization",
            format!("Bearer {execution_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_sandbox_manager(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = sandbox_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-sandbox");
        request.metadata_mut().insert(
            "x-sandbox-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_sandbox_manager(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn data_plane_credential_is_separate_identity_bound_and_never_ingress_authority() {
        let verifier = verifier("key-data-plane").await;
        let execution_token = sign(&claims(), "key-data-plane");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let missing = Request::new(());
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&missing, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut data_claims = claims();
        data_claims["aud"] = json!("data-plane");
        let data_token = sign(&data_claims, "key-data-plane");
        let mut delegated_request = Request::new(());
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {data_token}").parse().expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_data_plane(&delegated_request, &caller)
            .await
            .expect("delegated Data Plane bearer");
        assert_eq!(delegated.as_str(), data_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedDataPlaneBearer([REDACTED])"
        );

        // An aud=data-plane token must never become Execution Core authority.
        assert_eq!(
            verifier
                .authenticate(&authenticated_request(&data_token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        // An Execution Core token in the delegated header is the wrong audience.
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {execution_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&delegated_request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = data_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-data-plane");
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&delegated_request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn verifies_rs256_signature_and_canonical_user_identity() {
        let verifier = verifier("key-1").await;
        let token = sign(&claims(), "key-1");
        let user = verifier
            .authenticate(&authenticated_request(&token))
            .await
            .expect("verified user");

        assert_eq!(user.org_id, "org-1");
        assert_eq!(user.user_id, "user-1");
        assert!(user.zdr);
        assert!(!format!("{user:?}").contains(&token));
    }

    #[tokio::test]
    async fn signed_zdr_posture_is_required_and_delegated_posture_must_match() {
        let verifier = verifier("key-zdr").await;
        let mut missing_zdr = claims();
        missing_zdr
            .as_object_mut()
            .expect("claims object")
            .remove("zdr");
        let missing_token = sign(&missing_zdr, "key-zdr");
        assert_eq!(
            verifier
                .authenticate(&authenticated_request(&missing_token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let execution_token = sign(&claims(), "key-zdr");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");
        let mut delegated_claims = claims();
        delegated_claims["aud"] = json!("inference-core");
        delegated_claims["zdr"] = json!(false);
        let delegated_token = sign(&delegated_claims, "key-zdr");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {delegated_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_inference(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn rejects_forged_wrong_standard_claims_and_service_identity() {
        let verifier = verifier("key-2").await;
        assert_eq!(
            verifier
                .authenticate(&Request::new(()))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let unsigned = authenticated_request("attacker-controlled");
        assert_eq!(
            verifier.authenticate(&unsigned).await.unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        for (field, value) in [
            ("iss", json!("attacker")),
            ("aud", json!("model-gateway")),
            ("exp", json!(now() - 10)),
            ("nbf", json!(now() + 300)),
            ("org_id", json!("")),
            ("user_id", json!("user-2")),
            ("principal_type", json!("service")),
        ] {
            let mut invalid = claims();
            invalid[field] = value;
            let token = sign(&invalid, "key-2");
            assert_eq!(
                verifier
                    .authenticate(&authenticated_request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "field {field} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn scheduled_step_auth_accepts_only_signed_service_identity() {
        let verifier = verifier("key-scheduled-step").await;
        let mut service = claims();
        service["sub"] = json!("service:orchestrator-core");
        service["user_id"] = json!("");
        service["principal_type"] = json!("service");
        service["service_id"] = json!("service:orchestrator-core");
        service["scopes"] = json!(["model:schedule:step"]);
        let token = sign(&service, "key-scheduled-step");
        let identity = verifier
            .authenticate_scheduled_step(&authenticated_request(&token))
            .await
            .expect("signed service identity");
        assert_eq!(identity.service_id, "service:orchestrator-core");
        assert_eq!(identity.org_id, "org-1");
        assert!(identity.has_scope("model:schedule:step"));

        let mut user = claims();
        user["scopes"] = json!(["model:schedule:step"]);
        let user_token = sign(&user, "key-scheduled-step");
        assert_eq!(
            verifier
                .authenticate_scheduled_step(&authenticated_request(&user_token))
                .await
                .expect_err("user credential must not enter service lane")
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[tokio::test]
    async fn binds_request_tenant_and_user_to_signed_claims() {
        let verifier = verifier("key-3").await;
        let token = sign(&claims(), "key-3");
        let user = verifier
            .authenticate(&authenticated_request(&token))
            .await
            .expect("verified user");

        assert!(user.authorize("org-1", Some("user-1")).is_ok());
        assert_eq!(
            user.authorize("org-2", Some("user-1")).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            user.authorize("org-1", Some("user-2")).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            user.authorize("", Some("user-1")).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn rejects_undersized_and_malformed_rsa_jwks_keys() {
        let weak_modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff; 128]);
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1, 0, 1]);
        let weak = serde_json::to_vec(&json!({
            "keys": [{
                "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "weak",
                "n": weak_modulus, "e": exponent
            }]
        }))
        .expect("JWKS JSON");
        assert!(validate_jwks_document(&weak)
            .expect_err("1024-bit key must fail")
            .to_string()
            .contains("smaller than 2048 bits"));

        let malformed =
            br#"{"keys":[{"kty":"RSA","alg":"RS256","kid":"bad","n":"!!!","e":"AQAB"}]}"#;
        assert!(validate_jwks_document(malformed).is_err());
    }

    #[tokio::test]
    async fn concurrent_unknown_kid_requests_share_one_refresh() {
        let server = MockServer::start().await;
        let (_, n, e) = rsa_keypair();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": "known", "n": n, "e": e
                }]
            })))
            .expect(2)
            .mount(&server)
            .await;
        let verifier = JwtVerifier::from_config(AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: "data-plane".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier");
        let token = sign(&claims(), "unknown");

        let results = futures::future::join_all((0..12).map(|_| {
            let verifier = verifier.clone();
            let request = authenticated_request(&token);
            async move { verifier.authenticate(&request).await }
        }))
        .await;

        assert!(results.into_iter().all(|result| {
            result.expect_err("unknown key must fail").code() == tonic::Code::Unauthenticated
        }));
    }

    /// The claims of a persistent user token of a ZDR org, bound to one connection
    /// by a waiver (Z-18). The audience is the one the test presents it for.
    fn waived_claims(aud: &str) -> Value {
        let mut value = claims();
        value["aud"] = json!(aud);
        value["zdr"] = json!(false);
        value["org_zdr"] = json!(true);
        value["zdr_waiver"] = json!({
            "version": 1,
            "provider": "openai-codex-subscription",
            "connection": "conn-1",
            "ack_id": "ack-1",
        });
        value
    }

    fn with_bearer(header: &'static str, token: &str) -> Request<()> {
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert(header, format!("Bearer {token}").parse().expect("metadata"));
        request
    }

    #[tokio::test]
    async fn a_zdr_waiver_is_read_from_a_persistent_token_of_a_zdr_org() {
        let verifier = verifier("key-waiver").await;
        let token = sign(&waived_claims("execution-core"), "key-waiver");
        let user = verifier
            .authenticate(&authenticated_request(&token))
            .await
            .expect("a waiver the matrix allows verifies");
        assert!(!user.zdr, "a waiver rides a persistent token");
        assert_eq!(user.org_zdr, Some(true));
        let waiver = user.zdr_waiver.as_ref().expect("the waiver is carried");
        assert_eq!(waiver.version(), 1);
        assert_eq!(waiver.provider(), "openai-codex-subscription");
        assert_eq!(waiver.connection_id(), "conn-1");
        assert_eq!(waiver.ack_id(), "ack-1");
        assert!(!format!("{user:?}").contains(&token));
    }

    /// Every waiver the matrix does not allow is refused, never read as "no
    /// waiver": a token that silently dropped its waiver would be served as an
    /// ordinary persistent turn of a ZDR org (`e_org_zdr_without_waiver_is_not_persistent`).
    #[tokio::test]
    async fn malformed_or_misplaced_zdr_waivers_are_refused_not_dropped() {
        let verifier = verifier("key-waiver-refused").await;
        let mut cases: Vec<(&str, Value)> = Vec::new();
        cases.push(("unknown waiver version", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"]["version"] = json!(2);
            value
        }));
        cases.push(("a provider the waiver does not bind", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"]["provider"] = json!("azure-openai");
            value
        }));
        cases.push(("an empty connection", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"]["connection"] = json!("");
            value
        }));
        cases.push(("a padded connection", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"]["connection"] = json!(" conn-1 ");
            value
        }));
        cases.push(("a blank acknowledgement", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"]["ack_id"] = json!("  ");
            value
        }));
        cases.push(("a waiver that is not an object", {
            let mut value = waived_claims("execution-core");
            value["zdr_waiver"] = json!("conn-1");
            value
        }));
        cases.push(("a waiver on a ZDR token", {
            let mut value = waived_claims("execution-core");
            value["zdr"] = json!(true);
            value
        }));
        cases.push(("a waiver in an org that is not ZDR", {
            let mut value = waived_claims("execution-core");
            value["org_zdr"] = json!(false);
            value
        }));
        cases.push(("a waiver with no org_zdr claim", {
            let mut value = waived_claims("execution-core");
            value.as_object_mut().expect("object").remove("org_zdr");
            value
        }));
        cases.push(("a persistent token of a ZDR org with no waiver", {
            let mut value = waived_claims("execution-core");
            value.as_object_mut().expect("object").remove("zdr_waiver");
            value
        }));
        cases.push(("a malformed waiver in an org that is not ZDR", {
            let mut value = waived_claims("execution-core");
            value["org_zdr"] = json!(false);
            value["zdr_waiver"] = json!("conn-1");
            value
        }));
        for (label, value) in cases {
            let token = sign(&value, "key-waiver-refused");
            assert_eq!(
                verifier
                    .authenticate(&authenticated_request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "{label}"
            );
        }
    }

    /// A waiver is a decision about one human's connection: a service credential
    /// never carries one, whatever else it claims.
    #[tokio::test]
    async fn a_waiver_never_rides_a_service_credential() {
        let verifier = verifier("key-waiver-service").await;
        let now = now();
        let token = sign(
            &json!({
                "sub": "service:orchestrator-core",
                "iss": "auth-core",
                "aud": "execution-core",
                "exp": now + 3600,
                "nbf": now - 5,
                "org_id": "org-1",
                "service_id": "service:orchestrator-core",
                "principal_type": "service",
                "scopes": ["model:schedule:step"],
                "zdr": false,
                "org_zdr": true,
                "zdr_waiver": {
                    "version": 1,
                    "provider": "openai-codex-subscription",
                    "connection": "conn-1",
                    "ack_id": "ack-1",
                },
            }),
            "key-waiver-service",
        );
        assert_eq!(
            verifier
                .authenticate_scheduled_step(&authenticated_request(&token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    /// Every delegated bearer of a run carries the caller's own waiver. A bearer
    /// for another connection, or a persistent bearer with no waiver, is a
    /// posture mismatch on every audience (`e_bearers_waived_for_different_connections`
    /// and the mixed-set rows of the matrix).
    #[tokio::test]
    async fn delegated_bearers_must_carry_the_same_waiver_as_the_caller() {
        let verifier = verifier("key-waiver-delegated").await;
        let caller = verifier
            .authenticate(&authenticated_request(&sign(
                &waived_claims("execution-core"),
                "key-waiver-delegated",
            )))
            .await
            .expect("the waived caller verifies");

        let same_for = |aud: &str| sign(&waived_claims(aud), "key-waiver-delegated");
        let other_for = |aud: &str| {
            let mut value = waived_claims(aud);
            value["zdr_waiver"]["connection"] = json!("conn-2");
            sign(&value, "key-waiver-delegated")
        };
        let bare_for = |aud: &str| {
            let mut value = claims();
            value["aud"] = json!(aud);
            value["zdr"] = json!(false);
            sign(&value, "key-waiver-delegated")
        };

        // Inference.
        verifier
            .authenticate_delegated_inference(
                &with_bearer("x-inference-authorization", &same_for("inference-core")),
                &caller,
            )
            .await
            .expect("the same waiver is accepted");
        for token in [other_for("inference-core"), bare_for("inference-core")] {
            assert_eq!(
                verifier
                    .authenticate_delegated_inference(
                        &with_bearer("x-inference-authorization", &token),
                        &caller
                    )
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied,
                "inference"
            );
        }
        // Browser broker.
        verifier
            .authenticate_delegated_browser(
                &with_bearer("x-browser-authorization", &same_for("browser-broker")),
                &caller,
            )
            .await
            .expect("the same waiver is accepted");
        for token in [other_for("browser-broker"), bare_for("browser-broker")] {
            assert_eq!(
                verifier
                    .authenticate_delegated_browser(
                        &with_bearer("x-browser-authorization", &token),
                        &caller
                    )
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied,
                "browser"
            );
        }
        // Data Plane.
        verifier
            .authenticate_delegated_data_plane(
                &with_bearer("x-data-plane-authorization", &same_for("data-plane")),
                &caller,
            )
            .await
            .expect("the same waiver is accepted");
        for token in [other_for("data-plane"), bare_for("data-plane")] {
            assert_eq!(
                verifier
                    .authenticate_delegated_data_plane(
                        &with_bearer("x-data-plane-authorization", &token),
                        &caller
                    )
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied,
                "data-plane"
            );
        }
        // Sandbox manager.
        verifier
            .authenticate_delegated_sandbox_manager(
                &with_bearer("x-sandbox-authorization", &same_for("sandbox-manager")),
                &caller,
            )
            .await
            .expect("the same waiver is accepted");
        for token in [other_for("sandbox-manager"), bare_for("sandbox-manager")] {
            assert_eq!(
                verifier
                    .authenticate_delegated_sandbox_manager(
                        &with_bearer("x-sandbox-authorization", &token),
                        &caller
                    )
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied,
                "sandbox"
            );
        }
    }

    async fn bind_session(
        verifier: &JwtVerifier,
        token: &str,
        caller: &AuthenticatedUser,
    ) -> Result<(), Status> {
        let bearer = DelegatedSessionBearer(Arc::from(token));
        verifier.bind_session_bearer(&bearer, caller).await
    }

    /// The session bearer of a run is a user bearer of the same turn. It must carry
    /// the caller's own zdr and waiver, and its audience must be session-core, as the
    /// matrix requires of every bearer of a turn (`e_mixed_waiver_and_zdr_bearers`,
    /// `e_bearers_waived_for_different_connections`).
    #[tokio::test]
    async fn session_bearer_must_carry_the_same_waiver_as_the_caller() {
        let verifier = verifier("key-waiver-session").await;
        let caller = verifier
            .authenticate(&authenticated_request(&sign(
                &waived_claims("execution-core"),
                "key-waiver-session",
            )))
            .await
            .expect("the waived caller verifies");

        let same = sign(&waived_claims("session-core"), "key-waiver-session");
        bind_session(&verifier, &same, &caller)
            .await
            .expect("a session bearer with the same waiver is accepted");

        let mut other = waived_claims("session-core");
        other["zdr_waiver"]["connection"] = json!("conn-2");
        let mut zdr = waived_claims("session-core");
        zdr["zdr"] = json!(true);
        zdr.as_object_mut().expect("object").remove("zdr_waiver");
        let mut bare = claims();
        bare["aud"] = json!("session-core");
        bare["zdr"] = json!(false);
        for (label, value) in [
            ("another connection's waiver", other),
            ("a ZDR session", zdr),
            ("no waiver", bare),
        ] {
            let token = sign(&value, "key-waiver-session");
            assert_eq!(
                bind_session(&verifier, &token, &caller)
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied,
                "{label}"
            );
        }

        // A persistent caller with no waiver is bound by the zdr claim alone.
        let mut plain = claims();
        plain["aud"] = json!("execution-core");
        plain["zdr"] = json!(false);
        let plain_caller = verifier
            .authenticate(&authenticated_request(&sign(&plain, "key-waiver-session")))
            .await
            .expect("the persistent caller verifies");
        let mut zdr_session = claims();
        zdr_session["aud"] = json!("session-core");
        zdr_session["zdr"] = json!(true);
        let zdr_token = sign(&zdr_session, "key-waiver-session");
        assert_eq!(
            bind_session(&verifier, &zdr_token, &plain_caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied,
            "a ZDR session bearer is not a persistent caller's bearer"
        );

        let token = sign(&waived_claims("execution-core"), "key-waiver-session");
        assert_eq!(
            bind_session(&verifier, &token, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated,
            "a bearer minted for another audience is not a session bearer"
        );
    }
}
