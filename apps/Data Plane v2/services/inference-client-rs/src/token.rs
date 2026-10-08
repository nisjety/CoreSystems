//! The Auth Core service-token client for inference-core.
//!
//! A Data Plane worker (embedding, graph extraction) cannot forward a user's
//! bearer and must not hold a shared key that inference-core would accept for any
//! tenant. It asks Auth Core for a short-lived, audited `aud=inference-core`
//! service JWT for the organization whose content it is about to send, and
//! forwards only that bearer over gRPC.
//!
//! ## What this client guarantees
//!
//! * **The bearer is the one asked for.** The token's claims are checked against
//!   the request before it is used: audience, issuer, the service identity, the
//!   scope, the organization and the `reason` all have to be exactly what was
//!   asked, and the lifetime has to be bounded. Inference-core stays the
//!   cryptographic verifier; this is the bounded-response profile a client can
//!   check without the key.
//! * **The posture is the one configured, or the organization's own.** See
//!   [`RetentionPosture`].
//! * **A bearer is fetched per provider call, never held across a document.** The
//!   cache makes that cheap. A large document outlives a 60-second token, so one
//!   bearer taken at the start of a document would expire in the middle of it.
//! * **A cached bearer is reused only while it is fresh.** The cache is keyed by
//!   `(organization, reason)`: the reason is asserted on the claim and differs per
//!   feature, so a token minted for one feature is never served to another.
//!   Freshness is `min(60 s, ttl / 4)` before the expiry (15 s for a 60-second
//!   token, so reuse is at most 45 s), and for a tenant posture never more than
//!   45 s after the mint whatever the lifetime. That is this client's share of
//!   the staleness bound: a bearer minted just before an organization turned zero
//!   retention on is still served for up to 45 s. Auth Core adds its own share
//!   (it keeps an organization's posture for 45 s before it mints, and longer
//!   while org-core is unreachable), so the end-to-end figure to quote is Auth
//!   Core's disclosure in interactive-retention-policy.ts, not this one alone.
//! * **Concurrent callers share one mint.** Sixteen chunks of one document ask
//!   for the same bearer at the same moment; one of them goes to Auth Core and
//!   the others wait for that answer, success or failure.
//! * **A refusal to issue is a typed answer.** When Auth Core cannot resolve the
//!   organization's retention posture it answers 503 `retention_posture_unavailable`
//!   and issues no token. That is [`MintError::PostureUnavailable`], which decodes
//!   to a wait ([`crate::Disposition::Pending`]), never to "send" and never to
//!   "skip for good".

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::{BoxFuture, FutureExt, Shared};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use crate::clock::{Clock, SystemClock};
use crate::status::{Failure, InferenceError};

/// The audience every bearer this client accepts must carry.
pub const INFERENCE_AUDIENCE: &str = "inference-core";
/// The only scope this client requests.
pub const INFERENCE_SCOPE: &str = "inference:invoke";
/// The longest lifetime this client accepts on any token.
pub const MAX_TOKEN_TTL_SECONDS: i64 = 300;
/// The lifetime Auth Core gives a token minted for a `tenant` principal.
/// Informational: the client does not require it (Auth Core may change the cap),
/// it bounds reuse independently, see [`TENANT_MAX_REUSE`].
pub const TENANT_TOKEN_TTL_SECONDS: i64 = 60;
/// The most a bearer is reused before it is refreshed, however long it lives:
/// the point of the refresh margin is to keep the cache from serving a token
/// that is about to expire, and its cap keeps the figure small enough to quote.
pub const MAX_REFRESH_MARGIN: Duration = Duration::from_secs(60);
/// The longest a bearer minted under the `tenant` posture is reused. The
/// token's `zdr` claim is a snapshot of the organization's setting at the mint,
/// so this is the client's share of the staleness a ZDR switch is disclosed with
/// (Auth Core's own posture cache adds up to 45 s more before the mint). It equals
/// the reuse of a 60-second token with the default margin.
pub const TENANT_MAX_REUSE: Duration = Duration::from_secs(45);

const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;
const MAX_TOKEN_BYTES: usize = 8192;
const CLOCK_LEEWAY_SECONDS: i64 = 30;
const MIN_SERVICE_KEY_BYTES: usize = 16;
/// An entry per (organization, reason). Expired ones are dropped on insert once
/// the map reaches this size; with tokens that live a minute it is never close.
const MAX_CACHE_ENTRIES: usize = 4096;

/// The retention posture this deployment expects on an issued token.
///
/// The issuer alone selects the posture (Auth Core rejects any caller-supplied
/// `x-zdr` header or `zdr` body field), so this is an assertion that the token
/// matches the principal the service is registered as, never a request.
///
/// Keep it aligned with the registry's `retentionByAudience["inference-core"]`
/// entry for this service. A mismatch fails closed on the mint: that is how
/// embedding once died silently when the registry moved to `persistent` while the
/// clients still demanded `zdr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionPosture {
    /// Registry `"zdr"`: the token must carry `zdr:true`.
    ZeroRetention,
    /// Registry `"persistent"`: the token must carry `zdr:false`.
    Persistent,
    /// Registry `"tenant"`: the token's `zdr` is the ORGANIZATION's own setting,
    /// resolved by Auth Core at the mint. Either value is accepted; the caller
    /// reads it with [`Bearer::zdr`] and must act on it. A `zdr:true` bearer is
    /// refused by inference-core for every provider without a verified ZDR
    /// contract, which decodes to [`Failure::ZdrUnavailable`].
    ///
    /// Roll it out clients first: ship this client with the posture still
    /// `persistent`, then set the posture to `tenant`, and only then change the
    /// registry. A client that still expects `persistent` refuses the
    /// `zdr:true` token a tenant registry issues for a ZDR organization, which
    /// is the safe direction (nothing is sent), but it stalls that organization.
    Tenant,
}

impl RetentionPosture {
    /// Parse the value of `MODEL_PLANE_INFERENCE_RETENTION_POSTURE`.
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim() {
            "zdr" => Ok(Self::ZeroRetention),
            "persistent" => Ok(Self::Persistent),
            "tenant" => Ok(Self::Tenant),
            other => Err(ConfigError::Posture(other.to_owned())),
        }
    }

    /// The registry's vocabulary for it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZeroRetention => "zdr",
            Self::Persistent => "persistent",
            Self::Tenant => "tenant",
        }
    }

    /// Is a token carrying this `zdr` claim what this posture expects?
    pub const fn accepts(self, zdr: bool) -> bool {
        match self {
            Self::ZeroRetention => zdr,
            Self::Persistent => !zdr,
            Self::Tenant => true,
        }
    }
}

/// Renders an observed `zdr` claim in the registry's vocabulary so a mismatch
/// error names both sides in the terms the operator configures.
const fn observed_posture(zdr: bool) -> &'static str {
    if zdr {
        "zdr"
    } else {
        "persistent"
    }
}

/// Why a bearer is being requested. Asserted on the token's `reason` claim and
/// recorded in Auth Core's audit event, so an operator can read which feature a
/// token was for. One per feature: the cache never serves a token minted for one
/// feature to another.
///
/// The strings are the contract with Auth Core's golden
/// (`inference-service-tokens.golden.json`), whose `reasons` list a test compares
/// with [`Reason::ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Embedding the text of an indexed chunk.
    DocumentEmbedding,
    /// Generating a chunk's situating context before it is embedded.
    ContextualRetrieval,
    /// Describing a video segment's filmstrip in prose.
    VideoCaption,
    /// Extracting entities, relationships and claims for the knowledge graph.
    GraphExtraction,
    /// Summarising a graph community.
    GraphCommunitySummary,
}

impl Reason {
    pub const ALL: [Reason; 5] = [
        Reason::DocumentEmbedding,
        Reason::ContextualRetrieval,
        Reason::VideoCaption,
        Reason::GraphExtraction,
        Reason::GraphCommunitySummary,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentEmbedding => "document-embedding",
            Self::ContextualRetrieval => "contextual-retrieval",
            Self::VideoCaption => "video-caption",
            Self::GraphExtraction => "graph-extraction",
            Self::GraphCommunitySummary => "graph-community-summary",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A misconfigured client. The messages name the environment variables of the two
/// services (the container variables are the same in both), so a startup failure
/// says what to fix.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("MODEL_PLANE_INFERENCE_TOKEN_URL must be an absolute HTTP(S) URL")]
    TokenUrl,
    #[error(
        "MODEL_PLANE_INFERENCE_TOKEN_URL must be an uncredentialed HTTP(S) URL without query or fragment"
    )]
    TokenUrlShape,
    #[error("MODEL_PLANE_INFERENCE_TOKEN_ISSUER is required")]
    Issuer,
    #[error("MODEL_PLANE_INFERENCE_SERVICE_ID is invalid")]
    ServiceId,
    #[error("MODEL_PLANE_INFERENCE_SERVICE_API_KEY is missing or invalid")]
    ServiceApiKey,
    #[error(
        "MODEL_PLANE_INFERENCE_RETENTION_POSTURE must be `persistent`, `zdr` or `tenant`, got `{0}`"
    )]
    Posture(String),
    #[error("build inference service-token HTTP client: {0}")]
    Http(String),
}

/// Why a bearer could not be had. Cloneable, because concurrent callers that
/// joined one mint all receive its outcome.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MintError {
    #[error("inference tenant is invalid")]
    InvalidOrg,
    #[error("inference service principal credential is unavailable")]
    CredentialUnavailable,
    /// Auth Core could not resolve the organization's retention posture and
    /// issued no token (503 `retention_posture_unavailable`).
    #[error("inference service-token issuance deferred: the organization's retention posture could not be resolved (retention_posture_unavailable)")]
    PostureUnavailable,
    /// Auth Core could not tell whether the organization exists and issued no
    /// token (503 `organization_directory_unavailable`).
    #[error("inference service-token issuance deferred: the organization could not be verified (organization_directory_unavailable)")]
    DirectoryUnavailable,
    /// 401, 403 or 404: the credential, the principal, the organization or the
    /// audience is not accepted.
    #[error("inference service-token issuance refused with HTTP status {status}")]
    Refused { status: u16 },
    /// Any other 4xx except 429: the request was rejected as invalid.
    #[error("inference service-token request rejected with HTTP status {status}")]
    Rejected { status: u16 },
    /// 429, or a 5xx that is not one of the typed 503s.
    #[error("inference service-token issuance failed with HTTP status {status}")]
    ServerError { status: u16 },
    #[error("inference service-token endpoint unavailable: {0}")]
    Unavailable(String),
    #[error("inference service-token response was malformed: {0}")]
    Malformed(&'static str),
    /// The response parsed but the token or its claims were not what was asked.
    #[error("inference service-token {0}")]
    Contract(&'static str),
    /// The token's `zdr` claim is not what this deployment's posture expects.
    #[error(
        "inference service-token carried the `{observed}` retention posture but this deployment \
         expects `{expected}`; align MODEL_PLANE_INFERENCE_RETENTION_POSTURE with auth-core's \
         retentionByAudience entry for this principal's `inference-core` audience"
    )]
    PostureMismatch {
        observed: &'static str,
        expected: &'static str,
    },
}

impl MintError {
    /// The decoded failure class.
    pub fn failure(&self) -> Failure {
        match self {
            Self::InvalidOrg | Self::Rejected { .. } => Failure::InvalidArgument,
            Self::CredentialUnavailable
            | Self::Refused { .. }
            | Self::Contract(_)
            | Self::PostureMismatch { .. } => Failure::Auth,
            Self::PostureUnavailable => Failure::PostureUnavailable,
            Self::DirectoryUnavailable => Failure::DirectoryUnavailable,
            Self::ServerError { .. } | Self::Unavailable(_) | Self::Malformed(_) => {
                Failure::Transient { retry_after: None }
            }
        }
    }

    fn wire_kind(&self) -> Option<&'static str> {
        match self {
            Self::PostureUnavailable => Some("retention_posture_unavailable"),
            Self::DirectoryUnavailable => Some("organization_directory_unavailable"),
            _ => None,
        }
    }
}

impl From<MintError> for InferenceError {
    fn from(error: MintError) -> Self {
        InferenceError::from_mint(
            error.failure(),
            error.wire_kind().map(str::to_owned),
            error.to_string(),
        )
    }
}

impl From<&MintError> for InferenceError {
    fn from(error: &MintError) -> Self {
        error.clone().into()
    }
}

/// Where the token endpoint is and who the service is.
#[derive(Clone)]
pub struct TokenClientConfig {
    /// `MODEL_PLANE_INFERENCE_TOKEN_URL`.
    pub token_url: String,
    /// `MODEL_PLANE_INFERENCE_TOKEN_ISSUER`: the `iss` every token must carry.
    pub expected_issuer: String,
    /// `MODEL_PLANE_INFERENCE_SERVICE_ID`.
    pub service_id: String,
    /// `MODEL_PLANE_INFERENCE_SERVICE_API_KEY`. Never printed.
    pub service_api_key: String,
    /// `MODEL_PLANE_INFERENCE_RETENTION_POSTURE`.
    pub posture: RetentionPosture,
    /// Standalone startup may bind without Auth Core's registered principal. The
    /// client then stays unusable until a real credential is supplied, so a
    /// content request still fails closed rather than making an unauthenticated
    /// inference call.
    pub allow_unconfigured: bool,
}

impl fmt::Debug for TokenClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenClientConfig")
            .field("token_url", &self.token_url)
            .field("expected_issuer", &self.expected_issuer)
            .field("service_id", &self.service_id)
            .field("service_api_key", &"[REDACTED]")
            .field("posture", &self.posture)
            .field("allow_unconfigured", &self.allow_unconfigured)
            .finish()
    }
}

/// A bearer for one call to inference-core. Cheap to clone, never printed.
#[derive(Clone)]
pub struct Bearer {
    token: Arc<str>,
    zdr: bool,
    expires_at: DateTime<Utc>,
    reuse_until: DateTime<Utc>,
}

impl Bearer {
    /// The compact JWT, for an `authorization: Bearer ...` header.
    pub fn as_str(&self) -> &str {
        &self.token
    }

    /// The token's `zdr` claim: under the `tenant` posture, the organization's own
    /// setting when the token was minted. The caller must honour it.
    pub fn zdr(&self) -> bool {
        self.zdr
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// The last moment the cache will hand this bearer out again.
    pub fn reuse_until(&self) -> DateTime<Utc> {
        self.reuse_until
    }
}

impl fmt::Debug for Bearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bearer")
            .field("token", &"[REDACTED]")
            .field("zdr", &self.zdr)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Counters of what the cache did. The in-service evidence that a second mint
/// inside the window was served from memory: Auth Core's audit count cannot be
/// read cheaply, and with a 60-second token the window is short.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenStats {
    /// Served from the cache.
    pub hits: u64,
    /// Not in the cache: this call started a mint.
    pub misses: u64,
    /// Not in the cache, but a mint for the same key was already running; this
    /// call waited for it instead of making another.
    pub joined: u64,
    /// Requests that reached Auth Core. Equals `misses`.
    pub mints: u64,
    /// Mints that failed.
    pub failures: u64,
}

#[derive(Default)]
struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    joined: AtomicU64,
    mints: AtomicU64,
    failures: AtomicU64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    org_id: String,
    reason: Reason,
}

type Flight = Shared<BoxFuture<'static, Result<Bearer, MintError>>>;

enum Slot {
    Ready(Bearer),
    InFlight(Flight),
}

struct Inner {
    http: Client,
    token_url: Url,
    expected_issuer: Arc<str>,
    service_id: Arc<str>,
    service_api_key: Arc<str>,
    posture: RetentionPosture,
    clock: Arc<dyn Clock>,
    slots: Mutex<HashMap<CacheKey, Slot>>,
    counters: Counters,
}

impl Inner {
    fn slots(&self) -> MutexGuard<'_, HashMap<CacheKey, Slot>> {
        // The map holds plain data and every critical section is a few map
        // operations, so a poisoned lock leaves nothing half-written.
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The client. Clone it freely: clones share one cache.
#[derive(Clone)]
pub struct InferenceTokenClient {
    inner: Arc<Inner>,
}

impl fmt::Debug for InferenceTokenClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceTokenClient")
            .field("token_url", &self.inner.token_url)
            .field("expected_issuer", &self.inner.expected_issuer)
            .field("service_id", &self.inner.service_id)
            .field("service_api_key", &"[REDACTED]")
            .field("posture", &self.inner.posture)
            .finish()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest<'a> {
    org_id: &'a str,
    scopes: [&'static str; 1],
    reason: &'static str,
}

/// Deliberately tolerant of fields it does not know. The first copies of this
/// client rejected unknown fields, so every field the Control Plane adds to the
/// response would have failed every mint in every worker until each was rebuilt.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_at: String,
    expires_in_seconds: i64,
    issuer: String,
    audience: String,
}

#[derive(Deserialize)]
struct TokenClaims {
    iss: String,
    aud: String,
    sub: String,
    iat: i64,
    nbf: i64,
    exp: i64,
    org_id: String,
    principal_type: String,
    service_id: String,
    scopes: Vec<String>,
    reason: String,
    zdr: bool,
}

/// Auth Core's error body. Only the machine-readable `error` is read.
#[derive(Deserialize)]
struct ErrorBody {
    error: Option<String>,
}

impl InferenceTokenClient {
    /// Build a client that reads the wall clock.
    pub fn new(config: TokenClientConfig) -> Result<Self, ConfigError> {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    /// Build a client with an explicit clock. For tests that replay captured
    /// tokens and move time.
    pub fn with_clock(
        config: TokenClientConfig,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ConfigError> {
        let token_url = Url::parse(config.token_url.trim()).map_err(|_| ConfigError::TokenUrl)?;
        if !(matches!(token_url.scheme(), "http" | "https")
            && token_url.username().is_empty()
            && token_url.password().is_none()
            && token_url.query().is_none()
            && token_url.fragment().is_none())
        {
            return Err(ConfigError::TokenUrlShape);
        }
        let expected_issuer = config.expected_issuer;
        if expected_issuer.trim().is_empty() || expected_issuer != expected_issuer.trim() {
            return Err(ConfigError::Issuer);
        }
        if !valid_service_id(&config.service_id) {
            return Err(ConfigError::ServiceId);
        }
        let key = config.service_api_key;
        if !(config.allow_unconfigured || (key.len() >= MIN_SERVICE_KEY_BYTES && key == key.trim()))
        {
            return Err(ConfigError::ServiceApiKey);
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ConfigError::Http(error.to_string()))?;
        tracing::info!(
            service_id = %config.service_id,
            posture = config.posture.as_str(),
            token_url = %token_url,
            configured = key.len() >= MIN_SERVICE_KEY_BYTES,
            "inference token client ready"
        );
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                token_url,
                expected_issuer: Arc::from(expected_issuer),
                service_id: Arc::from(config.service_id),
                service_api_key: Arc::from(key),
                posture: config.posture,
                clock,
                slots: Mutex::new(HashMap::new()),
                counters: Counters::default(),
            }),
        })
    }

    pub fn posture(&self) -> RetentionPosture {
        self.inner.posture
    }

    pub fn stats(&self) -> TokenStats {
        let counters = &self.inner.counters;
        TokenStats {
            hits: counters.hits.load(Ordering::Relaxed),
            misses: counters.misses.load(Ordering::Relaxed),
            joined: counters.joined.load(Ordering::Relaxed),
            mints: counters.mints.load(Ordering::Relaxed),
            failures: counters.failures.load(Ordering::Relaxed),
        }
    }

    /// A bearer for one call to inference-core on behalf of `org_id`.
    ///
    /// Call it once per provider call, at the moment of the call. Do not keep the
    /// result across a document: it is a few dozen seconds old at most, and the
    /// next call gets a fresh one from the cache for free.
    pub async fn bearer(&self, org_id: &str, reason: Reason) -> Result<Bearer, MintError> {
        if !valid_org_id(org_id) {
            return Err(MintError::InvalidOrg);
        }
        if self.inner.service_api_key.len() < MIN_SERVICE_KEY_BYTES {
            return Err(MintError::CredentialUnavailable);
        }
        let key = CacheKey {
            org_id: org_id.to_owned(),
            reason,
        };
        let now = self.inner.clock.now();
        let flight = {
            let mut slots = self.inner.slots();
            match slots.get(&key) {
                Some(Slot::Ready(bearer)) if now < bearer.reuse_until => {
                    self.inner.counters.hits.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(org_id, %reason, "inference bearer served from cache");
                    return Ok(bearer.clone());
                }
                Some(Slot::InFlight(flight)) => {
                    self.inner.counters.joined.fetch_add(1, Ordering::Relaxed);
                    flight.clone()
                }
                _ => {
                    self.inner.counters.misses.fetch_add(1, Ordering::Relaxed);
                    if slots.len() >= MAX_CACHE_ENTRIES {
                        slots.retain(|_, slot| match slot {
                            Slot::Ready(bearer) => now < bearer.reuse_until,
                            Slot::InFlight(_) => true,
                        });
                    }
                    let flight = Self::start_flight(self.inner.clone(), key.clone());
                    slots.insert(key, Slot::InFlight(flight.clone()));
                    flight
                }
            }
        };
        flight.await
    }

    /// The mint, as a future every concurrent caller can await. The future itself
    /// records its outcome in the map before it resolves, so whichever caller
    /// drives it (the one that started it may have been cancelled) leaves the map
    /// consistent, and a caller that arrives after it resolved sees the result
    /// as a cache entry, not as a stale flight.
    fn start_flight(inner: Arc<Inner>, key: CacheKey) -> Flight {
        async move {
            let outcome = inner.mint(&key.org_id, key.reason).await;
            let mut slots = inner.slots();
            match &outcome {
                Ok(bearer) => {
                    slots.insert(key, Slot::Ready(bearer.clone()));
                }
                Err(_) => {
                    inner.counters.failures.fetch_add(1, Ordering::Relaxed);
                    slots.remove(&key);
                }
            }
            drop(slots);
            outcome
        }
        .boxed()
        .shared()
    }
}

impl Inner {
    async fn mint(&self, org_id: &str, reason: Reason) -> Result<Bearer, MintError> {
        self.counters.mints.fetch_add(1, Ordering::Relaxed);
        let response = self
            .http
            .post(self.token_url.clone())
            .header("x-service-id", self.service_id.as_ref())
            .header("x-service-api-key", self.service_api_key.as_ref())
            .json(&TokenRequest {
                org_id,
                scopes: [INFERENCE_SCOPE],
                reason: reason.as_str(),
            })
            .send()
            .await
            .map_err(|error| MintError::Unavailable(describe_transport_error(&error)))?;
        let status = response.status();
        if !status.is_success() {
            let body = read_bounded(response, MAX_ERROR_BODY_BYTES).await.ok();
            return Err(refusal_for(status.as_u16(), body.as_deref()));
        }
        let body = read_bounded(response, MAX_TOKEN_RESPONSE_BYTES).await?;
        let bundle: TokenResponse = serde_json::from_slice(&body)
            .map_err(|_| MintError::Malformed("the response is not a token bundle"))?;
        let bearer = self.validate_bundle(bundle, org_id, reason)?;
        tracing::info!(
            org_id,
            %reason,
            zdr = bearer.zdr,
            posture = self.posture.as_str(),
            expires_at = %bearer.expires_at,
            reuse_until = %bearer.reuse_until,
            "inference bearer minted"
        );
        Ok(bearer)
    }

    fn validate_bundle(
        &self,
        bundle: TokenResponse,
        org_id: &str,
        reason: Reason,
    ) -> Result<Bearer, MintError> {
        if !(bundle.audience == INFERENCE_AUDIENCE
            && bundle.issuer == self.expected_issuer.as_ref()
            && bundle.expires_in_seconds > 0
            && bundle.expires_in_seconds <= MAX_TOKEN_TTL_SECONDS
            && !bundle.token.is_empty()
            && bundle.token.len() <= MAX_TOKEN_BYTES
            && !bundle.token.chars().any(char::is_whitespace))
        {
            return Err(MintError::Contract(
                "response violated the bounded-token contract",
            ));
        }

        let header = decode_header(&bundle.token)
            .map_err(|_| MintError::Contract("was not a compact JWT"))?;
        if !(header.alg == Algorithm::RS256 && header.typ.as_deref() == Some("JWT")) {
            return Err(MintError::Contract("used an unsupported JWT profile"));
        }

        // Inference Core remains the cryptographic verifier. This local decode
        // only checks the bounded response profile before the bearer is used.
        // Time is checked below against the injected clock, not by the decoder
        // (which reads the wall clock): the claims must be present, their values
        // are judged here.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.insecure_disable_signature_validation();
        validation.set_audience(&[INFERENCE_AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
        validation.validate_exp = false;
        validation.validate_nbf = false;
        let claims =
            decode::<TokenClaims>(&bundle.token, &DecodingKey::from_secret(&[]), &validation)
                .map_err(|_| MintError::Contract("claims were invalid"))?
                .claims;

        let expected_subject = format!("service:{}", self.service_id);
        if !(claims.iss == self.expected_issuer.as_ref()
            && claims.aud == INFERENCE_AUDIENCE
            && claims.org_id == org_id
            && claims.principal_type == "service"
            && claims.sub == expected_subject
            && claims.service_id == expected_subject
            && claims.scopes.as_slice() == [INFERENCE_SCOPE]
            && claims.reason == reason.as_str())
        {
            return Err(MintError::Contract("claims exceeded requested authority"));
        }

        // Checked separately from the authority bounds above so a posture drift
        // between this deployment and Auth Core's registry names both sides
        // instead of hiding inside a nine-clause assertion.
        if !self.posture.accepts(claims.zdr) {
            return Err(MintError::PostureMismatch {
                observed: observed_posture(claims.zdr),
                expected: self.posture.as_str(),
            });
        }

        let now = self.clock.now();
        let expires_at = DateTime::parse_from_rfc3339(&bundle.expires_at)
            .map_err(|_| MintError::Contract("expiry was malformed"))?
            .with_timezone(&Utc);
        let remaining_claim_seconds = claims.exp - now.timestamp();
        let remaining_bundle_seconds = (expires_at - now).num_seconds();
        let not_in_the_future = claims.nbf <= now.timestamp() + CLOCK_LEEWAY_SECONDS
            && claims.iat <= now.timestamp() + CLOCK_LEEWAY_SECONDS;
        let bounded = claims.nbf <= claims.exp
            && claims.iat <= claims.exp
            && remaining_claim_seconds > 0
            && remaining_claim_seconds <= MAX_TOKEN_TTL_SECONDS
            && remaining_bundle_seconds > 0
            && remaining_bundle_seconds <= MAX_TOKEN_TTL_SECONDS
            && (claims.exp - expires_at.timestamp()).abs() <= 5
            && (bundle.expires_in_seconds - remaining_claim_seconds).abs() <= 5;
        if !(not_in_the_future && bounded) {
            return Err(MintError::Contract(
                "was expired or exceeded its maximum lifetime",
            ));
        }

        let lifetime = Duration::from_secs(bundle.expires_in_seconds.unsigned_abs());
        let margin = refresh_margin(lifetime);
        let mut reuse_until = expires_at - chrono_duration(margin);
        if self.posture == RetentionPosture::Tenant {
            // The `zdr` claim of a tenant bearer is a snapshot of the
            // organization's setting. Whatever lifetime Auth Core gave it, it is
            // not reused for longer than the disclosed staleness bound.
            reuse_until = reuse_until.min(now + chrono_duration(TENANT_MAX_REUSE));
        }
        Ok(Bearer {
            token: Arc::from(bundle.token),
            zdr: claims.zdr,
            expires_at,
            reuse_until,
        })
    }
}

/// How long before its expiry a bearer stops being reused: `min(60 s, ttl / 4)`.
///
/// A fixed 60-second margin would refresh a 60-second token on every call (the
/// tenant posture's lifetime), which defeats the cache and sends every chunk of a
/// document to Auth Core. A quarter of the lifetime keeps the reuse window at
/// three quarters of it: 45 s for a 60-second token, 240 s for a 300-second one.
pub fn refresh_margin(lifetime: Duration) -> Duration {
    (lifetime / 4).min(MAX_REFRESH_MARGIN)
}

fn chrono_duration(duration: Duration) -> chrono::Duration {
    chrono::Duration::from_std(duration).unwrap_or(chrono::Duration::zero())
}

/// What an error response from Auth Core means. The `error` code in the body is
/// what tells the two typed 503s apart; the status alone does not.
fn refusal_for(status: u16, body: Option<&[u8]>) -> MintError {
    let error_code = body
        .and_then(|bytes| serde_json::from_slice::<ErrorBody>(bytes).ok())
        .and_then(|parsed| parsed.error);
    match (status, error_code.as_deref()) {
        (503, Some("retention_posture_unavailable")) => MintError::PostureUnavailable,
        (503, Some("organization_directory_unavailable")) => MintError::DirectoryUnavailable,
        (401 | 403 | 404, _) => MintError::Refused { status },
        (429, _) | (500..=599, _) => MintError::ServerError { status },
        _ => MintError::Rejected { status },
    }
}

/// Reads a response body up to `limit` bytes. A body over the limit is an error
/// rather than a truncation: a truncated token is not a token.
async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, MintError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(MintError::Malformed("the response exceeded the size limit"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| MintError::Unavailable(describe_transport_error(&error)))?
    {
        if body.len() + chunk.len() > limit {
            return Err(MintError::Malformed("the response exceeded the size limit"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn describe_transport_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "timed out".to_owned()
    } else if error.is_connect() {
        "could not connect".to_owned()
    } else {
        "request failed".to_owned()
    }
}

fn valid_service_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars
            .next()
            .is_some_and(|value| value.is_ascii_alphanumeric())
        && chars.all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
}

fn valid_org_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars
            .next()
            .is_some_and(|value| value.is_ascii_alphanumeric())
        && chars
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | ':' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_posture_vocabulary_is_the_registrys() {
        assert_eq!(
            RetentionPosture::parse("persistent"),
            Ok(RetentionPosture::Persistent)
        );
        assert_eq!(
            RetentionPosture::parse(" zdr "),
            Ok(RetentionPosture::ZeroRetention)
        );
        assert_eq!(
            RetentionPosture::parse("tenant"),
            Ok(RetentionPosture::Tenant)
        );
        for bad in ["", "Tenant", "none", "on", "true"] {
            let error = RetentionPosture::parse(bad).expect_err(bad);
            assert!(
                error
                    .to_string()
                    .contains("MODEL_PLANE_INFERENCE_RETENTION_POSTURE"),
                "{error}"
            );
        }
        for posture in [
            RetentionPosture::Persistent,
            RetentionPosture::ZeroRetention,
            RetentionPosture::Tenant,
        ] {
            assert_eq!(RetentionPosture::parse(posture.as_str()), Ok(posture));
        }
    }

    #[test]
    fn each_posture_accepts_exactly_what_it_expects() {
        assert!(RetentionPosture::Persistent.accepts(false));
        assert!(!RetentionPosture::Persistent.accepts(true));
        assert!(RetentionPosture::ZeroRetention.accepts(true));
        assert!(!RetentionPosture::ZeroRetention.accepts(false));
        assert!(RetentionPosture::Tenant.accepts(true));
        assert!(RetentionPosture::Tenant.accepts(false));
    }

    #[test]
    fn reasons_are_distinct_and_within_inference_cores_bounds() {
        let mut seen = std::collections::HashSet::new();
        for reason in Reason::ALL {
            let text = reason.as_str();
            assert!(seen.insert(text), "{text} is used twice");
            // inference-core accepts a reason of 3..=500 trimmed characters.
            assert!((3..=500).contains(&text.len()), "{text}");
            assert_eq!(text, text.trim());
        }
    }

    #[test]
    fn the_refresh_margin_is_a_quarter_of_the_lifetime_capped_at_a_minute() {
        // 60 s tenant token: 15 s margin, reused for 45 s. The fixed 60 s margin
        // the first design had refreshed this token on every call.
        assert_eq!(
            refresh_margin(Duration::from_secs(60)),
            Duration::from_secs(15)
        );
        assert_eq!(
            refresh_margin(Duration::from_secs(300)),
            Duration::from_secs(60)
        );
        assert_eq!(
            refresh_margin(Duration::from_secs(240)),
            Duration::from_secs(60)
        );
        assert_eq!(
            refresh_margin(Duration::from_secs(120)),
            Duration::from_secs(30)
        );
        assert_eq!(
            refresh_margin(Duration::from_secs(2)),
            Duration::from_millis(500)
        );
        assert_eq!(refresh_margin(Duration::ZERO), Duration::ZERO);
        assert_eq!(
            Duration::from_secs(60) - refresh_margin(Duration::from_secs(60)),
            TENANT_MAX_REUSE,
            "the tenant reuse cap is what the default margin gives a 60 s token"
        );
    }

    #[test]
    fn an_http_refusal_is_told_apart_by_its_code_in_the_body() {
        let posture =
            br#"{"statusCode":503,"error":"retention_posture_unavailable","message":"x"}"#;
        let directory =
            br#"{"statusCode":503,"error":"organization_directory_unavailable","message":"x"}"#;
        assert_eq!(
            refusal_for(503, Some(posture)),
            MintError::PostureUnavailable
        );
        assert_eq!(
            refusal_for(503, Some(directory)),
            MintError::DirectoryUnavailable
        );
        // The same status without the code is an Auth Core that is down or cannot
        // issue, which is an outage, not a posture answer.
        assert_eq!(
            refusal_for(
                503,
                Some(
                    br#"{"statusCode":503,"message":"Service-principal issuance is unavailable"}"#
                )
            ),
            MintError::ServerError { status: 503 }
        );
        assert_eq!(
            refusal_for(503, None),
            MintError::ServerError { status: 503 }
        );
        // A typed code on another status is not honoured: only a 503 means "wait".
        assert_eq!(
            refusal_for(500, Some(posture)),
            MintError::ServerError { status: 500 }
        );
        assert_eq!(refusal_for(403, None), MintError::Refused { status: 403 });
        assert_eq!(refusal_for(401, None), MintError::Refused { status: 401 });
        assert_eq!(refusal_for(404, None), MintError::Refused { status: 404 });
        assert_eq!(
            refusal_for(429, None),
            MintError::ServerError { status: 429 }
        );
        assert_eq!(refusal_for(400, None), MintError::Rejected { status: 400 });
    }

    #[test]
    fn every_mint_failure_decodes_to_a_failure_with_a_disposition() {
        use crate::status::{Disposition, PENDING_RETRY_AFTER};
        let pending = InferenceError::from(MintError::PostureUnavailable);
        assert_eq!(pending.failure(), &Failure::PostureUnavailable);
        assert_eq!(
            pending.disposition(),
            Disposition::Pending {
                after: PENDING_RETRY_AFTER
            }
        );
        assert_eq!(pending.wire_kind(), Some("retention_posture_unavailable"));
        assert!(pending
            .to_string()
            .contains("retention_posture_unavailable"));
        assert_eq!(
            InferenceError::from(MintError::DirectoryUnavailable).failure(),
            &Failure::DirectoryUnavailable
        );
        for error in [
            MintError::CredentialUnavailable,
            MintError::Refused { status: 403 },
            MintError::Contract("claims were invalid"),
            MintError::PostureMismatch {
                observed: "zdr",
                expected: "persistent",
            },
        ] {
            assert_eq!(error.failure(), Failure::Auth, "{error}");
            assert_eq!(
                InferenceError::from(&error).disposition(),
                Disposition::Terminal,
                "{error}"
            );
        }
        for error in [
            MintError::Unavailable("timed out".to_owned()),
            MintError::ServerError { status: 502 },
            MintError::Malformed("x"),
        ] {
            assert_eq!(
                error.failure(),
                Failure::Transient { retry_after: None },
                "{error}"
            );
        }
        assert_eq!(MintError::InvalidOrg.failure(), Failure::InvalidArgument);
    }

    #[test]
    fn org_and_service_ids_are_validated_like_the_issuer_does() {
        assert!(valid_org_id("org-1"));
        assert!(valid_org_id("org:abc_1.2"));
        for bad in ["", " org", "org/1", "-org", "org id", &"a".repeat(129)] {
            assert!(!valid_org_id(bad), "{bad:?}");
        }
        assert!(valid_service_id("embedding-engine"));
        assert!(!valid_service_id(""));
        assert!(!valid_service_id("a:b"));
    }

    fn config(key: &str, allow_unconfigured: bool) -> TokenClientConfig {
        TokenClientConfig {
            token_url: "http://auth-core:3011/api/inference-core/internal-token".to_owned(),
            expected_issuer: "http://auth-core:3011/api/convex-auth".to_owned(),
            service_id: "embedding-engine".to_owned(),
            service_api_key: key.to_owned(),
            posture: RetentionPosture::Persistent,
            allow_unconfigured,
        }
    }

    #[test]
    fn a_missing_or_short_credential_fails_closed_and_names_the_variable() {
        for key in ["", "too-short"] {
            let error = InferenceTokenClient::new(config(key, false)).expect_err(key);
            assert_eq!(error, ConfigError::ServiceApiKey);
            assert!(error
                .to_string()
                .contains("MODEL_PLANE_INFERENCE_SERVICE_API_KEY"));
        }
    }

    #[test]
    fn standalone_startup_binds_without_a_credential_and_stays_unusable() {
        let client = InferenceTokenClient::new(config("", true))
            .expect("standalone startup may bind without the external principal");
        assert!(client.inner.service_api_key.is_empty());
    }

    #[test]
    fn the_token_url_must_be_a_plain_http_url() {
        for bad in [
            "",
            "auth-core:3011/x",
            "ftp://auth-core/x",
            "http://user:pass@auth-core/x",
            "http://auth-core/x?a=b",
            "http://auth-core/x#frag",
        ] {
            let mut cfg = config("service-key-0123456789", false);
            cfg.token_url = bad.to_owned();
            assert!(InferenceTokenClient::new(cfg).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_issuer_and_service_id_are_required() {
        let mut cfg = config("service-key-0123456789", false);
        cfg.expected_issuer = " ".to_owned();
        assert_eq!(
            InferenceTokenClient::new(cfg).expect_err("issuer"),
            ConfigError::Issuer
        );
        let mut cfg = config("service-key-0123456789", false);
        cfg.service_id = "bad id".to_owned();
        assert_eq!(
            InferenceTokenClient::new(cfg).expect_err("service id"),
            ConfigError::ServiceId
        );
    }

    #[test]
    fn a_bearer_never_prints_its_token() {
        // A bearer is a credential for inference-core for up to five minutes. It
        // travels through logs by accident (`{bearer:?}`, a struct that derives
        // Debug around it), so its own Debug must hold nothing of the token.
        let now = Utc::now();
        let bearer = Bearer {
            token: Arc::from("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.signature-that-must-not-print"),
            zdr: true,
            expires_at: now,
            reuse_until: now,
        };
        let printed = format!("{bearer:?}");
        assert!(
            !printed.contains("eyJ") && !printed.contains("signature-that"),
            "{printed}"
        );
        assert!(printed.contains("[REDACTED]"), "{printed}");
    }

    #[test]
    fn nothing_secret_reaches_a_debug_print() {
        let key = "service-key-0123456789-secret";
        let cfg = config(key, false);
        let client = InferenceTokenClient::new(cfg.clone()).expect("client");
        for printed in [format!("{cfg:?}"), format!("{client:?}")] {
            assert!(!printed.contains(key), "{printed}");
            assert!(printed.contains("[REDACTED]"), "{printed}");
        }
    }
}
