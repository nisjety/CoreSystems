//! Fail-closed Auth Core JWT verification for the Inference Core gRPC boundary.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use mp_contracts::agentic_subscription::PROVIDER_ID;
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Resolution, RouteView, WaiverView, SUBSCRIPTION_ALIASES,
};
use mp_contracts::zdr_refusal::Refusal;
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tonic::{Code, Request, Status};
use tracing::{error, warn};

const DEFAULT_JWKS_TTL_SECS: u64 = 300;
const DEFAULT_JWT_LEEWAY_SECS: u64 = 30;
const MAX_JWT_LEEWAY_SECS: u64 = 300;
const MAX_JWKS_BYTES: usize = 1_048_576;

/// `sub` of an `api_key` principal is this prefix plus its `key_id` (EP-12 S6). The shape rules
/// below are model-gateway's `is_well_formed_api_key` and auth-core's `apiKeyIdentityClaims`.
const API_KEY_SUBJECT_PREFIX: &str = "apikey:";
const MAX_API_KEY_ID_LEN: usize = 128;
const MAX_ACTING_USER_ID_LEN: usize = 256;
/// The one scope auth-core's API-key exchange grants for `aud=inference-core`. An `api_key`
/// principal invokes a provider with it; `inference:invoke` is never read from a key token.
const API_KEY_INVOKE_SCOPE: &str = "agent:runs";

/// `INFERENCE_API_KEY_PRINCIPAL`: admit the `api_key` principal type (EP-12 S6). Only the literal
/// `true` enables it; off, an `api_key` token is `unauthenticated` exactly as before.
fn api_key_principal_enabled(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Verifies independently signed user and workload credentials before a
/// request can reach a provider or the prompt cache.
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
    /// Whether an `api_key` token is a principal here (`INFERENCE_API_KEY_PRINCIPAL`).
    accept_api_key: bool,
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
            .field("accept_api_key", &self.accept_api_key)
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
    exp: u64,
    iat: u64,
    nbf: u64,
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    principal_type: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    zdr: bool,
    /// `org_zdr`: `true` when the caller's org is Zero Data Retention, as the issuer read it when it
    /// minted the token. Absent on service tokens and on tokens minted before the claim existed.
    /// Only the posture check reads it.
    #[serde(default)]
    org_zdr: Option<bool>,
    /// `zdr_waiver`: one user connection that runs with zero retention off for one turn. Minted only
    /// on a persistent user token of a ZDR org. A malformed waiver fails the whole token.
    #[serde(default)]
    zdr_waiver: Option<ZdrWaiverClaim>,
    /// `key_id`: the Better Auth key an `api_key` token was exchanged for. Read only on that type.
    #[serde(default)]
    key_id: Option<String>,
    /// `acting_user_id`: the key's owner on an `api_key` token, the entitlement subject.
    #[serde(default)]
    acting_user_id: Option<String>,
}

/// The wire shape of the `zdr_waiver` claim, the matrix's `waiver` object. Auth Core may add fields
/// (an acknowledgement id, a digest, a decision id); those are ignored. The three below are
/// required, so a waiver that names no provider or connection is refused, never read as no waiver.
#[derive(Debug, Clone, Deserialize)]
struct ZdrWaiverClaim {
    version: u32,
    provider: String,
    connection: String,
}

/// The connection-bound waiver of zero retention a verified user token carries (Z-16). Auth Core
/// stamps it only on a persistent token of a ZDR org, for one user connection that cannot give zero
/// retention and was acknowledged for the turn. It binds one provider and one connection, and the
/// boundary refuses it on any other route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZdrWaiver {
    version: u32,
    provider: String,
    connection: String,
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
    pub fn connection(&self) -> &str {
        &self.connection
    }
}

impl From<ZdrWaiverClaim> for ZdrWaiver {
    fn from(claim: ZdrWaiverClaim) -> Self {
        Self {
            version: claim.version,
            provider: claim.provider,
            connection: claim.connection,
        }
    }
}

/// The identity of a verified `api_key` principal (EP-12 S6, D2): the key owns the call, the person
/// who owns the key is the entitlement subject only. Neither is a user id, so nothing that reads
/// `user_id` treats a key as that person.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiKeyIdentity {
    key_id: String,
    acting_user_id: String,
}

/// Canonical identity derived only from a verified credential. The original
/// bearer is retained for a future scoped downstream delegation path and is
/// deliberately redacted from `Debug`.
#[derive(Clone)]
pub struct AuthenticatedPrincipal {
    pub org_id: String,
    pub user_id: Option<String>,
    pub subject: String,
    pub scopes: Vec<String>,
    issuer_zdr: bool,
    issuer_org_zdr: Option<bool>,
    zdr_waiver: Option<ZdrWaiver>,
    api_key: Option<ApiKeyIdentity>,
    bearer: Arc<str>,
}

impl fmt::Debug for AuthenticatedPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedPrincipal")
            .field("org_id", &self.org_id)
            .field("user_id", &self.user_id)
            .field("subject", &self.subject)
            .field("scopes", &self.scopes)
            .field("issuer_zdr", &self.issuer_zdr)
            .field("issuer_org_zdr", &self.issuer_org_zdr)
            .field("zdr_waiver", &self.zdr_waiver)
            .field("api_key", &self.api_key)
            .field("bearer", &"[REDACTED]")
            .finish()
    }
}

impl AuthenticatedPrincipal {
    #[cfg(test)]
    pub(crate) fn for_test(org_id: &str, user_id: Option<&str>, issuer_zdr: bool) -> Self {
        let subject = user_id.unwrap_or("service:test");
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.map(str::to_owned),
            subject: subject.to_owned(),
            scopes: vec!["inference:invoke".to_owned()],
            issuer_zdr,
            issuer_org_zdr: None,
            zdr_waiver: None,
            api_key: None,
            bearer: Arc::from("test-bearer"),
        }
    }

    /// A persistent user of a ZDR org, bound to one connection by a waiver (Z-16).
    #[cfg(test)]
    pub(crate) fn for_test_with_waiver(org_id: &str, user_id: &str, connection: &str) -> Self {
        Self {
            org_id: org_id.to_owned(),
            user_id: Some(user_id.to_owned()),
            subject: user_id.to_owned(),
            scopes: vec!["inference:invoke".to_owned()],
            issuer_zdr: false,
            issuer_org_zdr: Some(true),
            zdr_waiver: Some(ZdrWaiver {
                version: mp_contracts::retention::WAIVER_CONTRACT_VERSION,
                provider: PROVIDER_ID.to_owned(),
                connection: connection.to_owned(),
            }),
            api_key: None,
            bearer: Arc::from("test-bearer"),
        }
    }

    /// Bind a request tenant to the signed tenant. Empty tenant ids are not a
    /// wildcard because every provider operation is tenant-sensitive.
    ///
    /// # Errors
    /// Returns `invalid_argument` for an absent tenant and
    /// `permission_denied` for a cross-tenant request.
    #[allow(clippy::result_large_err)]
    pub fn authorize_org(&self, requested_org_id: &str) -> Result<(), Status> {
        if requested_org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if requested_org_id != self.org_id {
            warn!("inference request tenant does not match verified credential");
            return Err(Status::permission_denied("tenant access denied"));
        }
        Ok(())
    }

    /// Require provider-execution authority.
    ///
    /// # Errors
    /// Returns `permission_denied` when the verified credential lacks the
    /// exact `inference:invoke` scope.
    #[allow(clippy::result_large_err)]
    pub fn authorize_invoke(&self) -> Result<(), Status> {
        // A key token holds the Agent API scope its exchange granted, never `inference:invoke`.
        let required = if self.api_key.is_some() {
            API_KEY_INVOKE_SCOPE
        } else {
            "inference:invoke"
        };
        if self.scopes.iter().any(|scope| scope == required) {
            return Ok(());
        }
        warn!("verified principal lacks inference invocation scope");
        Err(Status::permission_denied("inference scope required"))
    }

    /// Permit catalogue reads to a dedicated read credential or to a caller
    /// already authorised to invoke a provider.
    ///
    /// # Errors
    /// Returns `permission_denied` when neither exact scope is present.
    #[allow(clippy::result_large_err)]
    pub fn authorize_catalog(&self) -> Result<(), Status> {
        // An API key reaches the chat turn only (EP-12 S6), so no catalogue read either.
        if self.api_key.is_some() {
            warn!("api key principal refused on an inference catalogue read");
            return Err(Status::permission_denied("inference scope required"));
        }
        if self
            .scopes
            .iter()
            .any(|scope| matches!(scope.as_str(), "inference:read" | "inference:invoke"))
        {
            return Ok(());
        }
        warn!("verified principal lacks inference catalogue scope");
        Err(Status::permission_denied("inference scope required"))
    }

    /// Require the dedicated routing-policy administration authority. Provider
    /// invocation does not imply control-plane mutation rights.
    ///
    /// # Errors
    /// Returns `permission_denied` unless the verified credential contains the
    /// exact `inference:policy:admin` scope.
    #[allow(clippy::result_large_err)]
    pub fn authorize_policy_admin(&self) -> Result<(), Status> {
        // Routing-policy administration is a person's or an operator's act, never a key's, even
        // if a key token were ever minted with the scope.
        if self.api_key.is_none()
            && self
                .scopes
                .iter()
                .any(|scope| scope == "inference:policy:admin")
        {
            return Ok(());
        }
        warn!("verified principal lacks inference routing-policy admin scope");
        Err(Status::permission_denied(
            "inference policy administration scope required",
        ))
    }

    /// Admit an `api_key` principal only on an RPC the caller names as a chat turn (`Infer`,
    /// `InferStream`; EP-12 S6). The caller decides `chat_turn` from the request type, so an RPC
    /// added later is refused to a key until it is listed. Every other principal passes.
    ///
    /// # Errors
    /// Returns `permission_denied` for an `api_key` principal on any other RPC.
    #[allow(clippy::result_large_err)]
    pub fn authorize_api_key_rpc(&self, chat_turn: bool) -> Result<(), Status> {
        if self.api_key.is_none() || chat_turn {
            return Ok(());
        }
        warn!("api key principal refused on an inference RPC outside the chat turn");
        Err(Status::permission_denied("api key principals may only run a chat turn"))
    }

    /// Whether this credential is an `api_key` principal.
    #[must_use]
    pub const fn is_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// The key id of an `api_key` principal, for attribution; `None` for every other principal.
    #[must_use]
    pub fn api_key_id(&self) -> Option<&str> {
        self.api_key.as_ref().map(|key| key.key_id.as_str())
    }

    /// The person whose entitlement applies to this call (WS10: Control decides, Model enforces at
    /// admission): the verified user, or the owner of an `api_key` principal. `None` is a service.
    #[must_use]
    pub fn entitlement_user_id(&self) -> Option<&str> {
        self.user_id
            .as_deref()
            .or_else(|| self.api_key.as_ref().map(|key| key.acting_user_id.as_str()))
    }

    /// Issuer-selected ZDR can only make request policy stricter.
    #[must_use]
    pub const fn effective_zdr(&self, request_zdr: bool) -> bool {
        self.issuer_zdr || request_zdr
    }

    /// The org's standing zero-retention posture, as the issuer read it when it minted the token.
    #[must_use]
    pub const fn org_zdr(&self) -> Option<bool> {
        self.issuer_org_zdr
    }

    /// The connection-bound waiver this credential carries, if any.
    #[must_use]
    pub fn zdr_waiver(&self) -> Option<&ZdrWaiver> {
        self.zdr_waiver.as_ref()
    }

    /// Refuse a chat call whose route the verified zero-retention posture does not allow (Z-16).
    ///
    /// This is the inference boundary's own check, so it holds when a caller in front of it is
    /// bypassed. A credential whose token carries no posture beyond `zdr` (a service credential, or
    /// a token minted before the org claim existed) is not checked here. Any other is decided by the
    /// shared resolver in `mp_contracts::retention`: a waiver is honoured only on the one provider
    /// and connection it binds and never on an explicit request, and a persistent token of a ZDR org
    /// without a waiver is never served.
    ///
    /// # Errors
    /// `permission_denied` for a waiver used off its route and for a token whose posture claims
    /// disagree; `failed_precondition` for the other ZDR refusals. Each carries its catalogue kind.
    #[allow(clippy::result_large_err)]
    pub fn enforce_zdr_route(
        &self,
        provider_hint: &str,
        connection_id: &str,
        request_zdr: bool,
    ) -> Result<(), Status> {
        // A connection belongs to a person; a key never runs on its owner's connection (EP-12 D2).
        if self.api_key.is_some() && !connection_id.is_empty() {
            warn!("api key principal refused on a user connection");
            return Err(Status::permission_denied(
                "api key principals cannot use a user connection",
            ));
        }
        if self.issuer_org_zdr.is_none() && self.zdr_waiver.is_none() {
            return Ok(());
        }
        let waiver = self.zdr_waiver.as_ref().map(|waiver| WaiverView {
            version: waiver.version,
            provider: &waiver.provider,
            connection: &waiver.connection,
        });
        let claims = ClaimsView {
            zdr: self.issuer_zdr,
            org_zdr: self.issuer_org_zdr,
            waiver,
        };
        let provider = canonical_route_provider(provider_hint);
        let route = RouteView {
            provider: &provider,
            connection: (!connection_id.is_empty()).then_some(connection_id),
            explicit_zdr: request_zdr,
        };
        match resolve_turn(&[claims], &route) {
            Resolution::Refuse(refusal) => {
                warn!(
                    code = refusal.code(),
                    "zero-retention posture refused at the inference boundary"
                );
                let code = if refusal == Refusal::WAIVER_ROUTE_MISMATCH
                    || refusal == Refusal::POSTURE_MISMATCH
                {
                    Code::PermissionDenied
                } else {
                    Code::FailedPrecondition
                };
                Err(refusal.grpc_status(code))
            }
            Resolution::Zdr { .. } | Resolution::Persistent { .. } | Resolution::Waived { .. } => {
                Ok(())
            }
        }
    }

    /// Refuse a call that is not a chat turn (Z-16). No connection is named, so no waiver covers it:
    /// a waived credential reaches no modality and no embedding, and a persistent token of a ZDR org
    /// is refused here as it is on every route.
    ///
    /// # Errors
    /// As [`Self::enforce_zdr_route`].
    #[allow(clippy::result_large_err)]
    pub fn enforce_non_chat_zdr_posture(&self, request_zdr: bool) -> Result<(), Status> {
        self.enforce_zdr_route("", "", request_zdr)
    }

    /// The user scope of a provider request (prompt cache, budget, capture). An `api_key` principal
    /// is scoped by its own subject `apikey:<key_id>`, so a key neither shares a cache entry with
    /// another key or a service nor passes for its owner.
    #[must_use]
    pub fn budget_user_id(&self) -> String {
        if self.api_key.is_some() {
            return self.subject.clone();
        }
        self.user_id.clone().unwrap_or_default()
    }

    #[must_use]
    pub fn bearer(&self) -> &str {
        &self.bearer
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
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("INFERENCE_CORE_AUTH_AUDIENCE")?,
            jwks_ttl: Duration::from_secs(optional_u64(
                "AUTH_CORE_JWKS_TTL_SECS",
                DEFAULT_JWKS_TTL_SECS,
            )),
            leeway_secs: optional_u64("AUTH_CORE_JWT_LEEWAY_SECS", DEFAULT_JWT_LEEWAY_SECS)
                .min(MAX_JWT_LEEWAY_SECS),
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
    /// Load auth configuration and eagerly validate the JWKS document. The
    /// listener is never bound when verification material is missing or bad.
    ///
    /// # Errors
    /// Returns an error for missing configuration, HTTP client construction,
    /// or an unavailable/malformed JWKS document.
    pub async fn from_env() -> anyhow::Result<Self> {
        let raw = std::env::var("INFERENCE_API_KEY_PRINCIPAL").ok();
        let accept_api_key = api_key_principal_enabled(raw.as_deref());
        Ok(Self::from_config(AuthConfig::from_env()?)
            .await?
            .with_api_key_principal(accept_api_key))
    }

    /// Admit or refuse the `api_key` principal type. [`Self::from_config`] refuses it.
    #[must_use]
    pub(crate) fn with_api_key_principal(mut self, accept: bool) -> Self {
        self.accept_api_key = accept;
        self
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
            accept_api_key: false,
        })
    }

    /// Cryptographically verify a gRPC bearer and return its canonical user or
    /// service identity. Only RS256 Auth Core tokens are accepted.
    ///
    /// # Errors
    /// Invalid credentials return `unauthenticated`; a JWKS refresh outage
    /// returns `unavailable` and remains fail closed.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate<T>(
        &self,
        request: &Request<T>,
    ) -> Result<AuthenticatedPrincipal, Status> {
        let token = extract_bearer(request)?;
        let header =
            decode_header(token).map_err(|_| Status::unauthenticated("invalid credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid credential"));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.trim().is_empty())
            .ok_or_else(|| Status::unauthenticated("invalid credential"))?;
        let jwk = self.jwk_for(&kid).await?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|error| {
            error!(%error, "Auth Core JWKS contains an unusable signing key");
            Status::unavailable("authentication verification unavailable")
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[self.audience.as_ref()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| Status::unauthenticated("invalid credential"))?
            .claims;
        validate_claims(claims, token, self.leeway_secs, self.accept_api_key)
    }

    async fn jwk_for(&self, kid: &str) -> Result<jsonwebtoken::jwk::Jwk, Status> {
        let observed_revision = {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid credential"));
                }
            }
            cache.revision
        };

        let _refresh = self.refresh_lock.lock().await;
        {
            let cache = self.cache.read().await;
            if cache.revision != observed_revision {
                return cache
                    .jwks
                    .find(kid)
                    .cloned()
                    .ok_or_else(|| Status::unauthenticated("invalid credential"));
            }
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid credential"));
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
        jwk.ok_or_else(|| Status::unauthenticated("invalid credential"))
    }
}

#[allow(clippy::result_large_err)]
fn extract_bearer<T>(request: &Request<T>) -> Result<&str, Status> {
    request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified credential required"))
}

#[allow(clippy::result_large_err)]
fn validate_claims(
    claims: Claims,
    token: &str,
    leeway_secs: u64,
    accept_api_key: bool,
) -> Result<AuthenticatedPrincipal, Status> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::unavailable("authentication clock unavailable"))?
        .as_secs();
    if claims.org_id.trim().is_empty()
        || claims.org_id.trim() != claims.org_id
        || claims.sub.trim().is_empty()
        || claims.sub.trim() != claims.sub
        || claims.iat > now.saturating_add(leeway_secs)
        || claims.nbf > claims.exp
        || claims.iat > claims.exp
        || claims
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || scope.trim() != scope || scope.len() > 200)
    {
        return Err(Status::unauthenticated("invalid credential"));
    }

    let user_id = match claims.principal_type.as_deref().unwrap_or("user") {
        "user"
            if !claims.user_id.is_empty()
                && claims.user_id.trim() == claims.user_id
                && claims.sub == claims.user_id
                && claims.service_id.is_none()
                && claims.reason.is_none() =>
        {
            Some(claims.user_id)
        }
        "service"
            if claims.user_id.is_empty()
                && claims.service_id.as_deref() == Some(claims.sub.as_str())
                && claims
                    .service_id
                    .as_deref()
                    .and_then(|id| id.strip_prefix("service:"))
                    .is_some_and(|id| !id.is_empty())
                && claims.reason.as_deref().is_some_and(|reason| {
                    reason == reason.trim() && (3..=500).contains(&reason.len())
                }) =>
        {
            None
        }
        // Only while INFERENCE_API_KEY_PRINCIPAL is on; off, the type falls to the refusal below.
        "api_key" if accept_api_key && is_well_formed_api_key(&claims) => None,
        _ => return Err(Status::unauthenticated("invalid credential")),
    };
    let api_key = if claims.principal_type.as_deref() == Some("api_key") {
        claims
            .key_id
            .clone()
            .zip(claims.acting_user_id.clone())
            .map(|(key_id, acting_user_id)| ApiKeyIdentity {
                key_id,
                acting_user_id,
            })
    } else {
        // While keys are admitted, a key id on any other type makes the token ambiguous.
        if accept_api_key && claims.key_id.is_some() {
            return Err(Status::unauthenticated("invalid credential"));
        }
        None
    };

    // A waiver decides one person's connection. A service credential or an API key has no
    // connection of its own, so it can never carry one.
    if claims.zdr_waiver.is_some() && user_id.is_none() {
        return Err(Status::unauthenticated("invalid credential"));
    }

    Ok(AuthenticatedPrincipal {
        org_id: claims.org_id,
        user_id,
        subject: claims.sub,
        scopes: claims.scopes,
        issuer_zdr: claims.zdr,
        issuer_org_zdr: claims.org_zdr,
        zdr_waiver: claims.zdr_waiver.map(ZdrWaiver::from),
        api_key,
        bearer: Arc::from(token),
    })
}

/// The `api_key` shape model-gateway and auth-core agree on: a bounded printable `key_id`,
/// `sub == "apikey:" + key_id`, an owner that is present, unpadded and not itself a key, and no
/// person or service attribute beside them.
fn is_well_formed_api_key(claims: &Claims) -> bool {
    let (Some(key_id), Some(owner)) = (claims.key_id.as_deref(), claims.acting_user_id.as_deref())
    else {
        return false;
    };
    (1..=MAX_API_KEY_ID_LEN).contains(&key_id.len())
        && key_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        && claims.sub.strip_prefix(API_KEY_SUBJECT_PREFIX) == Some(key_id)
        && claims.user_id.is_empty()
        && claims.service_id.is_none()
        && claims.reason.is_none()
        && !owner.is_empty()
        && owner.len() <= MAX_ACTING_USER_ID_LEN
        && owner == owner.trim()
        && !owner.starts_with(API_KEY_SUBJECT_PREFIX)
}

/// The provider a request names, spelled as the chain matches it: case, surrounding space and `_`
/// are ignored, and every spelling of the subscription route is that route's canonical id. Any
/// other name is kept as given, so no waiver matches it.
fn canonical_route_provider(provider_hint: &str) -> String {
    let folded = provider_hint.trim().to_ascii_lowercase().replace('_', "-");
    if folded == PROVIDER_ID || SUBSCRIPTION_ALIASES.contains(&folded.as_str()) {
        PROVIDER_ID.to_owned()
    } else {
        provider_hint.to_owned()
    }
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
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::thread_rng;
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
            let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
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
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
    }

    fn user_claims() -> Value {
        json!({
            "sub": "user-1", "iss": "auth-core", "aud": "inference-core",
            "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
            "org_id": "org-1", "user_id": "user-1", "principal_type": "user",
            "scopes": ["inference:invoke"], "zdr": true
        })
    }

    fn service_claims() -> Value {
        json!({
            "sub": "service:retrieval", "iss": "auth-core", "aud": "inference-core",
            "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
            "org_id": "org-1", "principal_type": "service",
            "service_id": "service:retrieval", "reason": "query embedding",
            "scopes": ["inference:invoke"], "zdr": true
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
            audience: "inference-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    fn request(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    #[tokio::test]
    async fn verifies_users_services_and_redacts_bearers() {
        let verifier = verifier("key-1").await;
        let user_token = sign(&user_claims(), "key-1");
        let user = verifier
            .authenticate(&request(&user_token))
            .await
            .expect("user");
        assert_eq!(user.org_id, "org-1");
        assert_eq!(user.user_id.as_deref(), Some("user-1"));
        assert_eq!(user.budget_user_id(), "user-1");
        assert_eq!(user.bearer(), user_token);
        assert!(user.effective_zdr(false));
        assert!(!format!("{user:?}").contains(&user_token));

        let service_token = sign(&service_claims(), "key-1");
        let service = verifier
            .authenticate(&request(&service_token))
            .await
            .expect("service");
        assert_eq!(service.user_id, None);
        assert_eq!(service.subject, "service:retrieval");
        assert_eq!(service.budget_user_id(), "");
    }

    /// WS16.S1: the second bearer a machine caller of the gateway's `/v1/ai/chat` presents.
    /// `service:quarry-edge` holds `inference:invoke` and `models:invoke`; the first authorises
    /// provider execution here, the second is what the gateway's route table checks upstream.
    #[tokio::test]
    async fn a_service_token_with_both_scopes_is_admitted() {
        let verifier = verifier("key-both").await;
        let mut claims = service_claims();
        claims["sub"] = json!("service:quarry-edge");
        claims["service_id"] = json!("service:quarry-edge");
        claims["reason"] = json!("extract structured data from page text");
        claims["scopes"] = json!(["inference:invoke", "models:invoke"]);
        claims["zdr"] = json!(false);
        let token = sign(&claims, "key-both");
        let principal = verifier
            .authenticate(&request(&token))
            .await
            .expect("a service token with both scopes authenticates");
        assert_eq!(principal.subject, "service:quarry-edge");
        assert_eq!(principal.user_id, None);
        assert!(principal.authorize_org("org-1").is_ok());
        assert!(principal.authorize_invoke().is_ok());
        assert!(principal.authorize_catalog().is_ok());
        assert!(!principal.effective_zdr(false), "a non-ZDR org stays non-ZDR");
        assert!(principal.effective_zdr(true), "the request can still make it stricter");
        // The same token minted for another org cannot be used for org-1's request.
        assert!(principal.authorize_org("org-2").is_err());

        // Holding only the gateway scope is not provider-execution authority.
        claims["scopes"] = json!(["models:invoke"]);
        let token = sign(&claims, "key-both");
        let principal = verifier
            .authenticate(&request(&token))
            .await
            .expect("it still authenticates");
        assert_eq!(
            principal.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn rejects_missing_forged_wrong_standard_and_ambiguous_claims() {
        let verifier = verifier("key-2").await;
        assert_eq!(
            verifier
                .authenticate(&Request::new(()))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        assert_eq!(
            verifier
                .authenticate(&request("attacker-controlled"))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        for (field, value) in [
            ("iss", json!("attacker")),
            ("aud", json!("model-gateway")),
            ("exp", json!(now() - 10)),
            ("iat", json!(now() + 300)),
            ("nbf", json!(now() + 300)),
            ("org_id", json!("")),
            ("user_id", json!("user-2")),
            ("principal_type", json!("service")),
        ] {
            let mut invalid = user_claims();
            invalid[field] = value;
            let token = sign(&invalid, "key-2");
            assert_eq!(
                verifier
                    .authenticate(&request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "field {field} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn pins_tenant_and_monotonically_enforces_zdr() {
        let verifier = verifier("key-3").await;
        let token = sign(&user_claims(), "key-3");
        let principal = verifier
            .authenticate(&request(&token))
            .await
            .expect("principal");
        assert!(principal.authorize_org("org-1").is_ok());
        assert_eq!(
            principal.authorize_org("org-2").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            principal.authorize_org("").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert!(principal.effective_zdr(false));
        assert!(principal.effective_zdr(true));
    }

    #[tokio::test]
    async fn enforces_invoke_and_catalog_scopes_with_least_privilege() {
        let verifier = verifier("key-scopes").await;

        let invoke = verifier
            .authenticate(&request(&sign(&user_claims(), "key-scopes")))
            .await
            .expect("invoke principal");
        assert!(invoke.authorize_invoke().is_ok());
        assert!(invoke.authorize_catalog().is_ok());

        let mut read_claims = user_claims();
        read_claims["scopes"] = json!(["inference:read"]);
        let read = verifier
            .authenticate(&request(&sign(&read_claims, "key-scopes")))
            .await
            .expect("read principal");
        assert!(read.authorize_catalog().is_ok());
        assert_eq!(
            read.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        let mut wrong_claims = user_claims();
        wrong_claims["scopes"] = json!(["cost:read"]);
        let wrong = verifier
            .authenticate(&request(&sign(&wrong_claims, "key-scopes")))
            .await
            .expect("wrong-scope principal remains authenticated");
        assert_eq!(
            wrong.authorize_catalog().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            wrong.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn routing_policy_admin_requires_its_own_exact_scope() {
        let verifier = verifier("key-policy-admin").await;

        let invoke = verifier
            .authenticate(&request(&sign(&user_claims(), "key-policy-admin")))
            .await
            .expect("invoke principal");
        assert_eq!(
            invoke.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        let mut admin_claims = user_claims();
        admin_claims["scopes"] = json!(["inference:policy:admin"]);
        let admin = verifier
            .authenticate(&request(&sign(&admin_claims, "key-policy-admin")))
            .await
            .expect("policy admin principal");
        assert!(admin.authorize_policy_admin().is_ok());

        let mut prefix_claims = user_claims();
        prefix_claims["scopes"] = json!(["inference:policy:admin:extra"]);
        let prefix = verifier
            .authenticate(&request(&sign(&prefix_claims, "key-policy-admin")))
            .await
            .expect("prefix principal");
        assert_eq!(
            prefix.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn unknown_key_ids_are_refreshed_once_then_rejected_from_negative_cache() {
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
            audience: "inference-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier");
        let token = sign(&user_claims(), "unknown");

        for _ in 0..2 {
            assert_eq!(
                verifier
                    .authenticate(&request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
    }

    #[test]
    fn rejects_weak_and_malformed_rsa_jwks_keys() {
        let weak_modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff; 128]);
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1, 0, 1]);
        let weak = serde_json::to_vec(&json!({
            "keys": [{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "weak",
                "n": weak_modulus, "e": exponent}]
        }))
        .expect("JWKS");
        assert!(validate_jwks_document(&weak)
            .expect_err("weak key")
            .to_string()
            .contains("smaller than 2048 bits"));
        assert!(validate_jwks_document(
            br#"{"keys":[{"kty":"RSA","alg":"RS256","kid":"bad","n":"!!!","e":"AQAB"}]}"#
        )
        .is_err());
    }

    #[tokio::test]
    async fn a_token_without_a_boolean_zdr_claim_is_rejected() {
        // Auth Core stamps zdr on every plane token. An absent or non-boolean
        // claim must not be read as persistent retention.
        let kid = "key-zdr-required";
        let verifier = verifier(kid).await;
        let mut without_claim = user_claims();
        without_claim
            .as_object_mut()
            .expect("claims object")
            .remove("zdr");
        assert_eq!(
            verifier
                .authenticate(&request(&sign(&without_claim, kid)))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let mut non_boolean = user_claims();
        non_boolean["zdr"] = json!("true");
        assert_eq!(
            verifier
                .authenticate(&request(&sign(&non_boolean, kid)))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        assert!(verifier
            .authenticate(&request(&sign(&user_claims(), kid)))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn a_zdr_waiver_claim_is_read_onto_the_principal() {
        let kid = "key-waiver-read";
        let verifier = verifier(kid).await;
        let mut claims = user_claims();
        claims["zdr"] = json!(false);
        claims["org_zdr"] = json!(true);
        claims["zdr_waiver"] = json!({
            "version": 1, "provider": "openai-codex-subscription", "connection": "conn-a",
            "ack_id": "ack-1", "digest": "digest-1", "decision_jti": "jti-1"
        });
        let principal = verifier
            .authenticate(&request(&sign(&claims, kid)))
            .await
            .expect("a waived user");
        assert!(!principal.effective_zdr(false));
        assert_eq!(principal.org_zdr(), Some(true));
        let waiver = principal.zdr_waiver().expect("the waiver is read");
        assert_eq!(
            (waiver.version(), waiver.provider(), waiver.connection()),
            (1, "openai-codex-subscription", "conn-a")
        );
        let plain = verifier
            .authenticate(&request(&sign(&user_claims(), kid)))
            .await
            .expect("a plain user");
        assert_eq!(plain.org_zdr(), None);
        assert!(plain.zdr_waiver().is_none());
    }

    #[tokio::test]
    async fn a_malformed_zdr_waiver_or_a_waiver_on_a_service_fails_the_token() {
        let kid = "key-waiver-malformed";
        let verifier = verifier(kid).await;
        let malformed = [
            json!({"version": 1, "provider": "openai-codex-subscription"}),
            json!({"version": 1, "connection": "conn-a"}),
            json!({"version": "1", "provider": "openai-codex-subscription", "connection": "conn-a"}),
            json!("conn-a"),
        ];
        for waiver in malformed {
            let mut claims = user_claims();
            claims["zdr"] = json!(false);
            claims["org_zdr"] = json!(true);
            claims["zdr_waiver"] = waiver.clone();
            assert_eq!(
                verifier
                    .authenticate(&request(&sign(&claims, kid)))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "{waiver}"
            );
        }
        // A service has no connection of its own, so it never carries a waiver.
        let mut service = service_claims();
        service["zdr"] = json!(false);
        service["org_zdr"] = json!(true);
        service["zdr_waiver"] =
            json!({"version": 1, "provider": "openai-codex-subscription", "connection": "conn-a"});
        assert_eq!(
            verifier
                .authenticate(&request(&sign(&service, kid)))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn a_waiver_reaches_only_the_route_and_connection_it_binds() {
        let principal = AuthenticatedPrincipal::for_test_with_waiver("org-1", "user-1", "conn-a");
        // The bound route, in any spelling the chain also matches, is allowed.
        for provider in [
            "openai-codex-subscription",
            " OPENAI_CODEX-SUBSCRIPTION ",
            "chatgpt-codex",
            "codex-subscription",
        ] {
            assert!(
                principal
                    .enforce_zdr_route(provider, "conn-a", false)
                    .is_ok(),
                "{provider:?} is the bound route"
            );
        }
        // Anything else is refused, typed as a route mismatch.
        for (provider, connection, explicit) in [
            ("openai-codex-subscription", "conn-b", false),
            ("openai-codex-subscription", "", false),
            ("openai-codex-subscription", "conn-a", true),
            ("azure-openai", "conn-a", false),
            ("", "conn-a", false),
            ("", "", false),
        ] {
            let status = principal
                .enforce_zdr_route(provider, connection, explicit)
                .expect_err("the waiver names one route");
            assert_eq!(
                status.code(),
                tonic::Code::PermissionDenied,
                "{provider:?} {connection:?} {explicit}"
            );
            assert_eq!(
                Refusal::from_status(&status),
                Some(Refusal::WAIVER_ROUTE_MISMATCH),
                "{provider:?} {connection:?} {explicit}"
            );
        }
    }

    #[test]
    fn a_waiver_covers_no_non_chat_call() {
        let principal = AuthenticatedPrincipal::for_test_with_waiver("org-1", "user-1", "conn-a");
        for request_zdr in [false, true] {
            let status = principal
                .enforce_non_chat_zdr_posture(request_zdr)
                .expect_err("no modality is the route the waiver binds");
            assert_eq!(
                Refusal::from_status(&status),
                Some(Refusal::WAIVER_ROUTE_MISMATCH)
            );
        }
    }

    #[test]
    fn a_persistent_token_of_a_zdr_org_without_a_waiver_is_refused_on_every_route() {
        let mut principal = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), false);
        principal.issuer_org_zdr = Some(true);
        for (provider, connection) in [
            ("openai-codex-subscription", "conn-a"),
            ("azure-openai", ""),
            ("", ""),
        ] {
            let status = principal
                .enforce_zdr_route(provider, connection, false)
                .expect_err("a persistent token of a ZDR org is never served");
            assert_eq!(status.code(), tonic::Code::PermissionDenied);
            assert_eq!(
                Refusal::from_status(&status),
                Some(Refusal::POSTURE_MISMATCH)
            );
        }
        assert!(principal.enforce_non_chat_zdr_posture(false).is_err());
    }

    #[test]
    fn a_zdr_token_of_a_zdr_org_never_reaches_a_connection_without_a_waiver() {
        let mut principal = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), true);
        principal.issuer_org_zdr = Some(true);
        let status = principal
            .enforce_zdr_route("openai-codex-subscription", "conn-a", false)
            .expect_err("a connection cannot give ZDR");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            Refusal::from_status(&status),
            Some(Refusal::CONNECTION_NOT_ACKNOWLEDGED)
        );
        // The default route of a ZDR turn is ZDR, so the posture check passes it on to the ZDR gates.
        assert!(principal.enforce_zdr_route("", "", false).is_ok());
        assert!(principal.enforce_non_chat_zdr_posture(false).is_ok());
    }

    #[test]
    fn a_token_without_posture_claims_is_not_checked_by_the_route_rule() {
        let principal = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), false);
        assert!(principal
            .enforce_zdr_route("openai-codex-subscription", "conn-a", false)
            .is_ok());
        assert!(principal.enforce_non_chat_zdr_posture(false).is_ok());
    }

    /// An `api_key` token as auth-core's API-key exchange mints it for `aud=inference-core`.
    fn api_key_claims() -> Value {
        json!({
            "sub": "apikey:key-1", "iss": "auth-core", "aud": "inference-core",
            "iat": now(), "nbf": now() - 5, "exp": now() + 120,
            "org_id": "org-1", "principal_type": "api_key",
            "key_id": "key-1", "acting_user_id": "user-1", "jti": "jti-1",
            "scopes": ["agent:runs"], "zdr": false
        })
    }

    async fn api_key_verifier(kid: &str) -> JwtVerifier {
        verifier(kid).await.with_api_key_principal(true)
    }

    #[tokio::test]
    async fn api_key_shape_is_exact() {
        let kid = "key-api-shape";
        let verifier = api_key_verifier(kid).await;
        let principal = verifier
            .authenticate(&request(&sign(&api_key_claims(), kid)))
            .await
            .expect("a well-formed api key token");
        assert!(principal.is_api_key());
        assert_eq!(principal.subject, "apikey:key-1");
        assert_eq!(principal.user_id, None);

        let long_key = "k".repeat(129);
        for (field, value) in [
            ("sub", json!("apikey:key-2")),
            ("sub", json!("key-1")),
            ("key_id", json!("")),
            ("key_id", json!("key 1")),
            ("key_id", json!(long_key)),
            ("key_id", Value::Null),
            ("acting_user_id", json!("")),
            ("acting_user_id", json!(" user-1")),
            ("acting_user_id", json!("apikey:key-9")),
            ("acting_user_id", json!("u".repeat(257))),
            ("acting_user_id", Value::Null),
            ("user_id", json!("user-1")),
            ("service_id", json!("service:x")),
            ("reason", json!("a reason a key never has")),
            ("principal_type", json!("apikey")),
        ] {
            let mut claims = api_key_claims();
            if value.is_null() {
                claims.as_object_mut().expect("claims object").remove(field);
            } else {
                claims[field] = value.clone();
            }
            assert_eq!(
                verifier
                    .authenticate(&request(&sign(&claims, kid)))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "{field} = {value}"
            );
        }

        // A key never carries a waiver, and a key id on a person's token is ambiguous.
        let mut waived = api_key_claims();
        waived["org_zdr"] = json!(true);
        waived["zdr_waiver"] =
            json!({"version": 1, "provider": "openai-codex-subscription", "connection": "conn-a"});
        let mut user_with_key = user_claims();
        user_with_key["key_id"] = json!("key-1");
        for claims in [waived, user_with_key] {
            assert_eq!(
                verifier
                    .authenticate(&request(&sign(&claims, kid)))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
    }

    #[tokio::test]
    async fn api_key_refused_when_switch_off() {
        let kid = "key-api-off";
        let verifier = verifier(kid).await;
        assert_eq!(
            verifier
                .authenticate(&request(&sign(&api_key_claims(), kid)))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        // Off, a user token is read exactly as before, a stray key id included.
        let mut user_with_key = user_claims();
        user_with_key["key_id"] = json!("key-1");
        let user = verifier
            .authenticate(&request(&sign(&user_with_key, kid)))
            .await
            .expect("unchanged while the switch is off");
        assert!(!user.is_api_key());
        assert_eq!(user.budget_user_id(), "user-1");
    }

    #[test]
    fn api_key_principal_parser_takes_only_literal_true() {
        assert!(api_key_principal_enabled(Some("true")));
        assert!(api_key_principal_enabled(Some(" true ")));
        for raw in [None, Some(""), Some("TRUE"), Some("1"), Some("false")] {
            assert!(!api_key_principal_enabled(raw), "{raw:?}");
        }
    }

    #[tokio::test]
    async fn api_key_only_reaches_infer() {
        let kid = "key-api-rpcs";
        let verifier = api_key_verifier(kid).await;
        let key = verifier
            .authenticate(&request(&sign(&api_key_claims(), kid)))
            .await
            .expect("api key");
        assert!(key.authorize_invoke().is_ok());
        assert!(key.authorize_org("org-1").is_ok());
        assert!(key.authorize_api_key_rpc(true).is_ok());
        assert_eq!(
            key.authorize_api_key_rpc(false).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            key.authorize_catalog().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            key.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        // Neither the person's invoke scope nor the admin scope widens a key.
        let mut widened = api_key_claims();
        widened["scopes"] = json!(["inference:invoke", "inference:policy:admin"]);
        let widened = verifier
            .authenticate(&request(&sign(&widened, kid)))
            .await
            .expect("still a key");
        assert_eq!(
            widened.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            widened.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        // The RPC gate leaves every other principal as it was.
        let user = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), false);
        assert!(user.authorize_api_key_rpc(false).is_ok());
    }

    #[tokio::test]
    async fn entitlement_subject_is_the_key_owner() {
        let kid = "key-api-entitlement";
        let verifier = api_key_verifier(kid).await;
        let key = verifier
            .authenticate(&request(&sign(&api_key_claims(), kid)))
            .await
            .expect("api key");
        assert_eq!(key.entitlement_user_id(), Some("user-1"));
        assert_eq!(key.api_key_id(), Some("key-1"));
        // The key is never the person: no user id, and its own scope for cache and budget.
        assert_eq!(key.user_id, None);
        assert_eq!(key.budget_user_id(), "apikey:key-1");

        let user = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), false);
        assert_eq!(user.entitlement_user_id(), Some("user-1"));
        assert_eq!(user.api_key_id(), None);
        let service = AuthenticatedPrincipal::for_test("org-1", None, false);
        assert_eq!(service.entitlement_user_id(), None);
    }

    #[tokio::test]
    async fn zdr_api_key_takes_the_zdr_route_or_is_refused() {
        let kid = "key-api-zdr";
        let verifier = api_key_verifier(kid).await;
        let mut zdr = api_key_claims();
        zdr["zdr"] = json!(true);
        let key = verifier
            .authenticate(&request(&sign(&zdr, kid)))
            .await
            .expect("a zdr key");
        // The token's ZDR holds whatever the request says.
        assert!(key.effective_zdr(false));
        assert!(key.effective_zdr(true));
        assert!(key.enforce_zdr_route("", "", false).is_ok());
        // A connection is never a key's route, ZDR or not.
        for claims in [zdr.clone(), api_key_claims()] {
            let key = verifier
                .authenticate(&request(&sign(&claims, kid)))
                .await
                .expect("a key");
            assert_eq!(
                key.enforce_zdr_route("openai-codex-subscription", "conn-a", false)
                    .unwrap_err()
                    .code(),
                tonic::Code::PermissionDenied
            );
        }
        // A persistent key token of a ZDR org is refused, never served as persistent.
        let mut mixed = api_key_claims();
        mixed["org_zdr"] = json!(true);
        let key = verifier
            .authenticate(&request(&sign(&mixed, kid)))
            .await
            .expect("a mixed-posture key");
        let status = key
            .enforce_zdr_route("", "", false)
            .expect_err("never downgraded");
        assert_eq!(
            Refusal::from_status(&status),
            Some(Refusal::POSTURE_MISMATCH)
        );
    }
}
