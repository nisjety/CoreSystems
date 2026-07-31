//! Optional Letta/agent-memory adapter.
//!
//! Session Core remains the durable source of truth. This adapter enriches
//! memory with semantic recall and agent-memory backends when configured.

use mp_contracts::model_plane::v1::{
    memory_service_client::MemoryServiceClient, DeleteMemoryRequest, IndexMemoryRequest,
    ListMemoryRequest, MemoryEntry, MemoryHealthRequest, SearchMemoryRequest,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, Endpoint},
    Request,
};
use tracing::{debug, warn};

/// Default per-call budget for letta-bridge. 900ms was unachievable: a semantic
/// memory search embeds the query (agent-memory-server calls Azure OpenAI) before
/// it can do the vector lookup, so the round trip is dominated by an inference
/// call. Live logs showed agent-memory-server returning `200 OK` while
/// session-core had already given up -- surfaced as `DEGRADED_LETTA_TIMEOUT` or a
/// gRPC `Cancelled`, i.e. work paid for and thrown away on every turn.
///
/// Env-tunable because the right value depends on embedding-provider latency,
/// which is deployment-specific.
const DEFAULT_LETTA_TIMEOUT_MS: u64 = 2_500;
/// Upper bound so a misconfiguration cannot stall a chat turn indefinitely --
/// memory is one context tier among several and must never own the turn's
/// latency.
const MAX_LETTA_TIMEOUT_MS: u64 = 10_000;

fn letta_timeout() -> Duration {
    let ms = std::env::var("LETTA_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_LETTA_TIMEOUT_MS)
        .min(MAX_LETTA_TIMEOUT_MS);
    Duration::from_millis(ms)
}
const AUTH_CORE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const AUTH_CORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;
const MAX_TOKEN_RESPONSE_BYTES: usize = 65_536;
const MAX_TOKEN_CACHE_ENTRIES: usize = 10_000;
const LETTA_AUDIENCE: &str = "letta-bridge";
const MEMORY_READ_SCOPE: &str = "memory:read";
const MEMORY_WRITE_SCOPE: &str = "memory:write";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum LettaReadiness {
    Unverified = 0,
    Ready = 1,
    AuthUnavailable = 2,
    MetadataRejected = 3,
    RpcUnavailable = 4,
    Timeout = 5,
    LexicalFallback = 6,
    SemanticUnverified = 7,
    SemanticUnavailable = 8,
    Unknown = 9,
}

impl LettaReadiness {
    fn load(value: u8) -> Self {
        match value {
            1 => Self::Ready,
            2 => Self::AuthUnavailable,
            3 => Self::MetadataRejected,
            4 => Self::RpcUnavailable,
            5 => Self::Timeout,
            6 => Self::LexicalFallback,
            7 => Self::SemanticUnverified,
            8 => Self::SemanticUnavailable,
            9 => Self::Unknown,
            _ => Self::Unverified,
        }
    }

    fn from_bridge_status(status: &str) -> Self {
        match status {
            "OK" => Self::Ready,
            "DEGRADED_LEXICAL_FALLBACK" => Self::LexicalFallback,
            "DEGRADED_SEMANTIC_UNVERIFIED" => Self::SemanticUnverified,
            "DEGRADED_SEMANTIC_UNAVAILABLE" => Self::SemanticUnavailable,
            _ => Self::Unknown,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Unverified => "DEGRADED_LETTA_UNVERIFIED",
            Self::Ready => "OK",
            Self::AuthUnavailable => "DEGRADED_LETTA_AUTH_UNAVAILABLE",
            Self::MetadataRejected => "DEGRADED_LETTA_METADATA_REJECTED",
            Self::RpcUnavailable => "DEGRADED_LETTA_UNAVAILABLE",
            Self::Timeout => "DEGRADED_LETTA_TIMEOUT",
            Self::LexicalFallback => "DEGRADED_LETTA_LEXICAL_FALLBACK",
            Self::SemanticUnverified => "DEGRADED_LETTA_SEMANTIC_UNVERIFIED",
            Self::SemanticUnavailable => "DEGRADED_LETTA_SEMANTIC_UNAVAILABLE",
            Self::Unknown => "DEGRADED_LETTA_STATUS_UNKNOWN",
        }
    }

    const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LettaHealthSnapshot {
    pub(crate) ready: bool,
    pub(crate) status: &'static str,
}

#[derive(Debug)]
pub(crate) struct LettaSearchOutcome {
    pub(crate) entries: Vec<MemoryEntry>,
    pub(crate) degradation_reason: Option<&'static str>,
}

#[derive(Debug)]
pub(crate) struct LettaIndexOutcome {
    pub(crate) memory_id: Option<String>,
    pub(crate) degradation_reason: Option<&'static str>,
}

#[derive(Debug)]
pub(crate) struct LettaListOutcome {
    pub(crate) entries: Vec<MemoryEntry>,
    pub(crate) degradation_reason: Option<&'static str>,
}

#[derive(Debug)]
pub(crate) struct LettaDeleteOutcome {
    // Not read by the current caller (memory_grpc::delete_memory only cares
    // whether the semantic-side delete is degraded, since the durable record
    // is the source of truth for existence) but kept, and exercised by tests,
    // so a future caller that needs to distinguish "deleted" from "was never
    // there" does not have to change this outcome's shape.
    #[allow(dead_code)]
    pub(crate) deleted: bool,
    pub(crate) degradation_reason: Option<&'static str>,
}

#[derive(Debug, thiserror::Error)]
enum LettaAuthError {
    #[error("{0} is required when Letta memory is configured")]
    MissingConfiguration(&'static str),
    #[error("invalid Letta caller configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("Auth Core token request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Auth Core refused the Letta caller credential ({0})")]
    Refused(reqwest::StatusCode),
    #[error("Auth Core returned an invalid Letta caller credential")]
    InvalidResponse,
}

struct LettaClientConfig {
    memory_endpoint: String,
    auth_core_url: String,
    service_id: String,
    credential: String,
}

impl fmt::Debug for LettaClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LettaClientConfig")
            .field("memory_endpoint", &self.memory_endpoint)
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl LettaClientConfig {
    fn from_env() -> Result<Option<Self>, LettaAuthError> {
        let memory_endpoint =
            optional_env("LETTA_MEMORY_ADDR").or_else(|| optional_env("LETTA_MEMORY_URL"));
        Self::new(
            memory_endpoint.as_deref(),
            optional_env("AUTH_CORE_URL").as_deref(),
            optional_env("SESSION_CORE_SERVICE_ID").as_deref(),
            optional_env("SESSION_CORE_SERVICE_API_KEY").as_deref(),
        )
    }

    fn new(
        memory_endpoint: Option<&str>,
        auth_core_url: Option<&str>,
        service_id: Option<&str>,
        credential: Option<&str>,
    ) -> Result<Option<Self>, LettaAuthError> {
        let Some(memory_endpoint) = non_empty(memory_endpoint) else {
            return Ok(None);
        };
        let auth_core_url = required(auth_core_url, "AUTH_CORE_URL")?;
        let service_id = required(service_id, "SESSION_CORE_SERVICE_ID")?;
        let credential = required(credential, "SESSION_CORE_SERVICE_API_KEY")?;

        let memory_endpoint = normalized_service_url(memory_endpoint, "LETTA_MEMORY_ADDR")?;
        let auth_core_url = normalized_service_url(auth_core_url, "AUTH_CORE_URL")?;
        if !valid_service_id(service_id) {
            return Err(LettaAuthError::InvalidConfiguration(
                "SESSION_CORE_SERVICE_ID",
            ));
        }
        if credential.len() < 16 {
            return Err(LettaAuthError::InvalidConfiguration(
                "SESSION_CORE_SERVICE_API_KEY",
            ));
        }

        Ok(Some(Self {
            memory_endpoint,
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
        }))
    }
}

fn optional_env(name: &'static str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn required<'a>(value: Option<&'a str>, name: &'static str) -> Result<&'a str, LettaAuthError> {
    non_empty(value).ok_or(LettaAuthError::MissingConfiguration(name))
}

fn normalized_service_url(value: &str, name: &'static str) -> Result<String, LettaAuthError> {
    let normalized = if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    let parsed =
        reqwest::Url::parse(&normalized).map_err(|_| LettaAuthError::InvalidConfiguration(name))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(parsed.path(), "" | "/")
    {
        return Err(LettaAuthError::InvalidConfiguration(name));
    }
    Ok(normalized)
}

fn valid_service_id(value: &str) -> bool {
    (2..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TokenCacheKey {
    org_id: String,
    scope: String,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct LettaTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<TokenCacheKey, CachedToken>>>,
    mint_lock: Arc<Mutex<()>>,
}

impl fmt::Debug for LettaTokenProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LettaTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    audience: String,
}

impl LettaTokenProvider {
    fn new(config: &LettaClientConfig) -> Result<Self, LettaAuthError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(AUTH_CORE_CONNECT_TIMEOUT)
            .timeout(AUTH_CORE_REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            auth_core_url: config.auth_core_url.clone(),
            service_id: config.service_id.clone(),
            credential: config.credential.clone(),
            http,
            cache: Arc::new(Mutex::new(HashMap::new())),
            mint_lock: Arc::new(Mutex::new(())),
        })
    }

    #[cfg(test)]
    fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(AUTH_CORE_REQUEST_TIMEOUT)
                .build()
                .expect("test HTTP client"),
            cache: Arc::new(Mutex::new(HashMap::new())),
            mint_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn token(&self, org_id: &str, scopes: &[&str]) -> Result<String, LettaAuthError> {
        let key = token_cache_key(org_id, scopes)?;
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, cached| cached.expires_at > now + TOKEN_REFRESH_SKEW);
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.token.clone());
            }
        }

        // A token miss is uncommon and bounded by the Auth Core TTL. Serialize
        // misses so concurrent background/search calls cannot stampede the
        // audited token-issuance endpoint; re-check after taking the lock.
        let _mint_guard = self.mint_lock.lock().await;
        let now = Instant::now();
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache
                .get(&key)
                .filter(|cached| cached.expires_at > now + TOKEN_REFRESH_SKEW)
            {
                return Ok(cached.token.clone());
            }
        }
        let response = self.mint(&key).await?;
        let token = response.token;
        let mut cache = self.cache.lock().await;
        cache.retain(|_, cached| cached.expires_at > now + TOKEN_REFRESH_SKEW);
        if cache.len() >= MAX_TOKEN_CACHE_ENTRIES {
            if let Some(eviction_key) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.expires_at)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&eviction_key);
            }
        }
        cache.insert(
            key,
            CachedToken {
                token: token.clone(),
                expires_at: now + Duration::from_secs(response.expires_in_seconds),
            },
        );
        Ok(token)
    }

    async fn mint(&self, key: &TokenCacheKey) -> Result<TokenResponse, LettaAuthError> {
        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| LettaAuthError::InvalidConfiguration("SESSION_CORE_SERVICE_API_KEY"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/{LETTA_AUDIENCE}/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": &key.org_id,
                "scopes": [&key.scope],
                "reason": "session-core Letta memory access",
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(LettaAuthError::Refused(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
        {
            return Err(LettaAuthError::InvalidResponse);
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(LettaAuthError::InvalidResponse);
        }
        let bundle: TokenResponse =
            serde_json::from_slice(&bytes).map_err(|_| LettaAuthError::InvalidResponse)?;
        if bundle.audience != LETTA_AUDIENCE
            || bundle.token.trim().is_empty()
            || bundle.token.chars().any(char::is_whitespace)
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            return Err(LettaAuthError::InvalidResponse);
        }
        Ok(bundle)
    }
}

fn token_cache_key(org_id: &str, scopes: &[&str]) -> Result<TokenCacheKey, LettaAuthError> {
    let org_id = org_id.trim();
    if org_id.is_empty() || org_id.chars().any(char::is_control) {
        return Err(LettaAuthError::InvalidConfiguration("Letta organization"));
    }
    let [scope] = scopes else {
        return Err(LettaAuthError::InvalidConfiguration("Letta token scope"));
    };
    if !matches!(*scope, MEMORY_READ_SCOPE | MEMORY_WRITE_SCOPE) {
        return Err(LettaAuthError::InvalidConfiguration("Letta token scope"));
    }
    Ok(TokenCacheKey {
        org_id: org_id.to_owned(),
        scope: (*scope).to_owned(),
    })
}

fn authenticated_request<T>(value: T, bearer: &str) -> Result<Request<T>, LettaAuthError> {
    let mut authorization = MetadataValue::try_from(format!("Bearer {bearer}"))
        .map_err(|_| LettaAuthError::InvalidResponse)?;
    authorization.set_sensitive(true);
    let mut request = Request::new(value);
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

#[derive(Clone)]
pub(crate) struct LettaMemoryAdapter {
    client: MemoryServiceClient<Channel>,
    tokens: LettaTokenProvider,
    readiness: Arc<AtomicU8>,
}

impl LettaMemoryAdapter {
    pub(crate) fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(config) = LettaClientConfig::from_env()? else {
            return Ok(None);
        };
        let endpoint = Endpoint::from_shared(config.memory_endpoint.clone())?
            .connect_timeout(AUTH_CORE_CONNECT_TIMEOUT)
            .timeout(letta_timeout());
        let tokens = LettaTokenProvider::new(&config)?;
        debug!(
            addr = %config.memory_endpoint,
            audience = LETTA_AUDIENCE,
            service_id = %config.service_id,
            "authenticated Letta memory adapter configured"
        );
        Ok(Some(Self {
            client: MemoryServiceClient::new(endpoint.connect_lazy()),
            tokens,
            readiness: Arc::new(AtomicU8::new(LettaReadiness::Unverified as u8)),
        }))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        memory_endpoint: &str,
        auth_core_url: &str,
        service_id: &str,
        credential: &str,
    ) -> Self {
        let endpoint = Endpoint::from_shared(memory_endpoint.to_owned()).expect("memory endpoint");
        Self {
            client: MemoryServiceClient::new(endpoint.connect_lazy()),
            tokens: LettaTokenProvider::new_for_test(auth_core_url, service_id, credential),
            readiness: Arc::new(AtomicU8::new(LettaReadiness::Unverified as u8)),
        }
    }

    fn set_readiness(&self, readiness: LettaReadiness) {
        self.readiness.store(readiness as u8, Ordering::Release);
    }

    #[must_use]
    pub(crate) fn health_snapshot(&self) -> LettaHealthSnapshot {
        let readiness = LettaReadiness::load(self.readiness.load(Ordering::Acquire));
        LettaHealthSnapshot {
            ready: readiness.is_ready(),
            status: readiness.code(),
        }
    }

    async fn probe_health(&self, bearer: &str) -> LettaReadiness {
        let mut client = self.client.clone();
        let Ok(request) = authenticated_request(MemoryHealthRequest {}, bearer) else {
            return LettaReadiness::MetadataRejected;
        };
        match tokio::time::timeout(letta_timeout(), client.health(request)).await {
            Ok(Ok(response)) => {
                LettaReadiness::from_bridge_status(response.into_inner().status.trim())
            }
            Ok(Err(_)) => LettaReadiness::RpcUnavailable,
            Err(_) => LettaReadiness::Timeout,
        }
    }

    fn search_degraded(readiness: LettaReadiness) -> LettaSearchOutcome {
        LettaSearchOutcome {
            entries: Vec::new(),
            degradation_reason: Some(readiness.code()),
        }
    }

    fn index_degraded(readiness: LettaReadiness) -> LettaIndexOutcome {
        LettaIndexOutcome {
            memory_id: None,
            degradation_reason: Some(readiness.code()),
        }
    }

    fn list_degraded(readiness: LettaReadiness) -> LettaListOutcome {
        LettaListOutcome {
            entries: Vec::new(),
            degradation_reason: Some(readiness.code()),
        }
    }

    fn delete_degraded(readiness: LettaReadiness) -> LettaDeleteOutcome {
        LettaDeleteOutcome {
            deleted: false,
            degradation_reason: Some(readiness.code()),
        }
    }

    pub(crate) async fn index(
        &self,
        org_id: &str,
        thread_id: &str,
        topic: &str,
        content: &str,
        user_id: Option<&str>,
        memory_id: Option<&str>,
    ) -> Option<String> {
        self.index_detailed(org_id, thread_id, topic, content, user_id, memory_id)
            .await
            .memory_id
    }

    /// `user_id` tags the entry with its owner when it is user-scoped, and
    /// `memory_id` — when the caller supplies one — is reused verbatim as the
    /// identifier on the semantic backend, so a later `delete_detailed` with
    /// that same id removes the same logical memory on both the durable index
    /// and the semantic store. When `memory_id` is empty the backend assigns
    /// its own id and the two stores are not correlated.
    pub(crate) async fn index_detailed(
        &self,
        org_id: &str,
        thread_id: &str,
        topic: &str,
        content: &str,
        user_id: Option<&str>,
        memory_id: Option<&str>,
    ) -> LettaIndexOutcome {
        let mut client = self.client.clone();
        let token = match self.tokens.token(org_id, &[MEMORY_WRITE_SCOPE]).await {
            Ok(token) => token,
            Err(error) => {
                self.set_readiness(LettaReadiness::AuthUnavailable);
                warn!(
                    %error,
                    audience = LETTA_AUDIENCE,
                    operation = "index",
                    "Letta memory caller credential unavailable; remaining degraded"
                );
                return Self::index_degraded(LettaReadiness::AuthUnavailable);
            }
        };
        let request = match authenticated_request(
            IndexMemoryRequest {
                thread_id: thread_id.to_owned(),
                topic: topic.to_owned(),
                content: content.to_owned(),
                org_id: org_id.to_owned(),
                user_id: user_id.unwrap_or_default().to_owned(),
                memory_id: memory_id.unwrap_or_default().to_owned(),
            },
            &token,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.set_readiness(LettaReadiness::MetadataRejected);
                warn!(%error, operation = "index", "Letta caller metadata rejected");
                return Self::index_degraded(LettaReadiness::MetadataRejected);
            }
        };

        match tokio::time::timeout(letta_timeout(), client.index_memory(request)).await {
            Ok(Ok(response)) => {
                let readiness = self.probe_health(&token).await;
                self.set_readiness(readiness);
                LettaIndexOutcome {
                    memory_id: Some(response.into_inner().memory_id),
                    degradation_reason: (!readiness.is_ready()).then_some(readiness.code()),
                }
            }
            Ok(Err(error)) => {
                self.set_readiness(LettaReadiness::RpcUnavailable);
                warn!(code = ?error.code(), "Letta memory index degraded");
                Self::index_degraded(LettaReadiness::RpcUnavailable)
            }
            Err(_) => {
                self.set_readiness(LettaReadiness::Timeout);
                warn!("Letta memory index timed out");
                Self::index_degraded(LettaReadiness::Timeout)
            }
        }
    }

    pub(crate) async fn search_detailed(
        &self,
        org_id: &str,
        thread_id: &str,
        query: &str,
        topic_filter: &[String],
        limit: u32,
    ) -> LettaSearchOutcome {
        let mut client = self.client.clone();
        let token = match self.tokens.token(org_id, &[MEMORY_READ_SCOPE]).await {
            Ok(token) => token,
            Err(error) => {
                self.set_readiness(LettaReadiness::AuthUnavailable);
                warn!(
                    %error,
                    audience = LETTA_AUDIENCE,
                    operation = "search",
                    "Letta memory caller credential unavailable; remaining degraded"
                );
                return Self::search_degraded(LettaReadiness::AuthUnavailable);
            }
        };
        let request = match authenticated_request(
            SearchMemoryRequest {
                thread_id: thread_id.to_owned(),
                query: query.to_owned(),
                topic_filter: topic_filter.to_vec(),
                limit,
                org_id: org_id.to_owned(),
                updated_after: None,
            },
            &token,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.set_readiness(LettaReadiness::MetadataRejected);
                warn!(%error, operation = "search", "Letta caller metadata rejected");
                return Self::search_degraded(LettaReadiness::MetadataRejected);
            }
        };

        match tokio::time::timeout(letta_timeout(), client.search_memory(request)).await {
            Ok(Ok(response)) => {
                let readiness = self.probe_health(&token).await;
                self.set_readiness(readiness);
                LettaSearchOutcome {
                    entries: response.into_inner().entries,
                    degradation_reason: (!readiness.is_ready()).then_some(readiness.code()),
                }
            }
            Ok(Err(error)) => {
                self.set_readiness(LettaReadiness::RpcUnavailable);
                warn!(code = ?error.code(), "Letta memory search degraded");
                Self::search_degraded(LettaReadiness::RpcUnavailable)
            }
            Err(_) => {
                self.set_readiness(LettaReadiness::Timeout);
                warn!("Letta memory search timed out");
                Self::search_degraded(LettaReadiness::Timeout)
            }
        }
    }

    /// Lists semantic memory entries owned by a user, across every thread.
    /// Uses the same `MemoryService` contract as search but is never
    /// thread-scoped -- see `ListMemoryRequest` in `memory.proto`.
    pub(crate) async fn list_detailed(
        &self,
        org_id: &str,
        user_id: &str,
        limit: u32,
    ) -> LettaListOutcome {
        let mut client = self.client.clone();
        let token = match self.tokens.token(org_id, &[MEMORY_READ_SCOPE]).await {
            Ok(token) => token,
            Err(error) => {
                self.set_readiness(LettaReadiness::AuthUnavailable);
                warn!(
                    %error,
                    audience = LETTA_AUDIENCE,
                    operation = "list",
                    "Letta memory caller credential unavailable; remaining degraded"
                );
                return Self::list_degraded(LettaReadiness::AuthUnavailable);
            }
        };
        let request = match authenticated_request(
            ListMemoryRequest {
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                limit,
            },
            &token,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.set_readiness(LettaReadiness::MetadataRejected);
                warn!(%error, operation = "list", "Letta caller metadata rejected");
                return Self::list_degraded(LettaReadiness::MetadataRejected);
            }
        };

        match tokio::time::timeout(letta_timeout(), client.list_memory(request)).await {
            Ok(Ok(response)) => {
                let readiness = self.probe_health(&token).await;
                self.set_readiness(readiness);
                LettaListOutcome {
                    entries: response.into_inner().entries,
                    degradation_reason: (!readiness.is_ready()).then_some(readiness.code()),
                }
            }
            Ok(Err(error)) => {
                self.set_readiness(LettaReadiness::RpcUnavailable);
                warn!(code = ?error.code(), "Letta memory list degraded");
                Self::list_degraded(LettaReadiness::RpcUnavailable)
            }
            Err(_) => {
                self.set_readiness(LettaReadiness::Timeout);
                warn!("Letta memory list timed out");
                Self::list_degraded(LettaReadiness::Timeout)
            }
        }
    }

    /// Deletes a single semantic memory entry by id. Best-effort: the caller
    /// (`memory_grpc::delete_memory`) always removes the durable index record
    /// regardless of this outcome, since the durable record is the source of
    /// truth for authorization and existence.
    pub(crate) async fn delete_detailed(
        &self,
        org_id: &str,
        user_id: &str,
        memory_id: &str,
    ) -> LettaDeleteOutcome {
        let mut client = self.client.clone();
        let token = match self.tokens.token(org_id, &[MEMORY_WRITE_SCOPE]).await {
            Ok(token) => token,
            Err(error) => {
                self.set_readiness(LettaReadiness::AuthUnavailable);
                warn!(
                    %error,
                    audience = LETTA_AUDIENCE,
                    operation = "delete",
                    "Letta memory caller credential unavailable; remaining degraded"
                );
                return Self::delete_degraded(LettaReadiness::AuthUnavailable);
            }
        };
        let request = match authenticated_request(
            DeleteMemoryRequest {
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                memory_id: memory_id.to_owned(),
            },
            &token,
        ) {
            Ok(request) => request,
            Err(error) => {
                self.set_readiness(LettaReadiness::MetadataRejected);
                warn!(%error, operation = "delete", "Letta caller metadata rejected");
                return Self::delete_degraded(LettaReadiness::MetadataRejected);
            }
        };

        match tokio::time::timeout(letta_timeout(), client.delete_memory(request)).await {
            Ok(Ok(response)) => {
                let readiness = self.probe_health(&token).await;
                self.set_readiness(readiness);
                LettaDeleteOutcome {
                    deleted: response.into_inner().deleted,
                    degradation_reason: (!readiness.is_ready()).then_some(readiness.code()),
                }
            }
            Ok(Err(error)) => {
                self.set_readiness(LettaReadiness::RpcUnavailable);
                warn!(code = ?error.code(), "Letta memory delete degraded");
                Self::delete_degraded(LettaReadiness::RpcUnavailable)
            }
            Err(_) => {
                self.set_readiness(LettaReadiness::Timeout);
                warn!("Letta memory delete timed out");
                Self::delete_degraded(LettaReadiness::Timeout)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn mount_read_token(auth: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/api/letta-bridge/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "letta-read-token",
                "expiresInSeconds": 300,
                "audience": "letta-bridge"
            })))
            .expect(1)
            .mount(auth)
            .await;
    }

    #[test]
    fn configured_memory_requires_dedicated_service_principal_settings() {
        assert!(LettaClientConfig::new(None, None, None, None)
            .expect("disabled adapter")
            .is_none());

        for (auth_core_url, service_id, credential) in [
            (None, Some("session-core"), Some("credential")),
            (Some("http://auth-core:3011"), None, Some("credential")),
            (Some("http://auth-core:3011"), Some("session-core"), None),
        ] {
            let error = LettaClientConfig::new(
                Some("http://letta-bridge:9096"),
                auth_core_url,
                service_id,
                credential,
            )
            .expect_err("configured Letta must fail closed without caller auth");
            assert!(error.to_string().contains("required"));
        }
    }

    #[tokio::test]
    async fn mints_exact_audience_tenant_and_scope_without_memory_content() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/letta-bridge/internal-token"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-api-key", "session-core-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["memory:read"],
                "reason": "session-core Letta memory access"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "letta-read-token",
                "expiresInSeconds": 300,
                "audience": "letta-bridge"
            })))
            .expect(1)
            .mount(&auth)
            .await;

        let provider =
            LettaTokenProvider::new_for_test(&auth.uri(), "session-core", "session-core-secret");
        assert_eq!(
            provider.token("org-a", &["memory:read"]).await.unwrap(),
            "letta-read-token"
        );
        assert_eq!(
            provider.token("org-a", &["memory:read"]).await.unwrap(),
            "letta-read-token"
        );
    }

    #[tokio::test]
    async fn rejects_wrong_audience_and_does_not_fabricate_a_credential() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/letta-bridge/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-plane-token",
                "expiresInSeconds": 300,
                "audience": "session-core"
            })))
            .mount(&auth)
            .await;

        let provider = LettaTokenProvider::new_for_test(&auth.uri(), "session-core", "secret");
        let error = provider
            .token("org-a", &["memory:write"])
            .await
            .expect_err("wrong audience must fail closed");
        assert!(error.to_string().contains("invalid"));
    }

    #[test]
    fn bearer_is_attached_only_as_sensitive_grpc_metadata() {
        let request = authenticated_request(SearchMemoryRequest::default(), "signed-letta-token")
            .expect("metadata");
        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer signed-letta-token")
        );
        assert!(format!("{request:?}").contains("Sensitive"));
        assert!(!format!("{request:?}").contains("signed-letta-token"));
    }

    #[test]
    fn bridge_readiness_is_normalized_to_stable_session_statuses() {
        for (bridge, expected, ready) in [
            ("OK", "OK", true),
            (
                "DEGRADED_LEXICAL_FALLBACK",
                "DEGRADED_LETTA_LEXICAL_FALLBACK",
                false,
            ),
            (
                "DEGRADED_SEMANTIC_UNVERIFIED",
                "DEGRADED_LETTA_SEMANTIC_UNVERIFIED",
                false,
            ),
            (
                "DEGRADED_SEMANTIC_UNAVAILABLE",
                "DEGRADED_LETTA_SEMANTIC_UNAVAILABLE",
                false,
            ),
            (
                "unexpected-provider-state",
                "DEGRADED_LETTA_STATUS_UNKNOWN",
                false,
            ),
        ] {
            let readiness = LettaReadiness::from_bridge_status(bridge);
            assert_eq!(readiness.code(), expected);
            assert_eq!(readiness.is_ready(), ready);
        }
    }

    #[tokio::test]
    async fn auth_core_failure_is_degraded_not_an_empty_success() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/letta-bridge/internal-token"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&auth)
            .await;
        let adapter = LettaMemoryAdapter::new_for_test(
            "http://127.0.0.1:9",
            &auth.uri(),
            "session-core",
            "session-core-test-credential",
        );

        let outcome = adapter
            .search_detailed("org-a", "thread-a", "query", &[], 5)
            .await;

        assert!(outcome.entries.is_empty());
        assert_eq!(
            outcome.degradation_reason,
            Some("DEGRADED_LETTA_AUTH_UNAVAILABLE")
        );
        assert_eq!(
            adapter.health_snapshot(),
            LettaHealthSnapshot {
                ready: false,
                status: "DEGRADED_LETTA_AUTH_UNAVAILABLE",
            }
        );
    }

    #[tokio::test]
    async fn rpc_failure_is_degraded_not_an_empty_success() {
        let auth = MockServer::start().await;
        mount_read_token(&auth).await;
        let unavailable =
            std::net::TcpListener::bind("127.0.0.1:0").expect("reserve local endpoint");
        let endpoint = format!(
            "http://{}",
            unavailable.local_addr().expect("local address")
        );
        drop(unavailable);

        let adapter = LettaMemoryAdapter::new_for_test(
            &endpoint,
            &auth.uri(),
            "session-core",
            "session-core-test-credential",
        );
        let outcome = adapter
            .search_detailed("org-a", "thread-a", "query", &[], 5)
            .await;

        assert!(outcome.entries.is_empty());
        assert_eq!(
            outcome.degradation_reason,
            Some("DEGRADED_LETTA_UNAVAILABLE")
        );
        assert_eq!(
            adapter.health_snapshot(),
            LettaHealthSnapshot {
                ready: false,
                status: "DEGRADED_LETTA_UNAVAILABLE",
            }
        );
    }

    #[tokio::test]
    async fn timeout_is_degraded_not_an_empty_success() {
        let auth = MockServer::start().await;
        mount_read_token(&auth).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stalled endpoint");
        let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
        let stalled = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("accept client");
            std::future::pending::<()>().await;
        });

        let adapter = LettaMemoryAdapter::new_for_test(
            &endpoint,
            &auth.uri(),
            "session-core",
            "session-core-test-credential",
        );
        let outcome = adapter
            .search_detailed("org-a", "thread-a", "query", &[], 5)
            .await;
        stalled.abort();

        assert!(outcome.entries.is_empty());
        assert_eq!(outcome.degradation_reason, Some("DEGRADED_LETTA_TIMEOUT"));
        assert_eq!(
            adapter.health_snapshot(),
            LettaHealthSnapshot {
                ready: false,
                status: "DEGRADED_LETTA_TIMEOUT",
            }
        );
    }
}
