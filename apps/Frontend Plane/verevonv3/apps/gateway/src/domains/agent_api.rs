//! EP-12 S8: the public ingress of the Agent API v1 (`/api/agent/v1/*`).
//!
//! An external caller sends an `x-api-key` header and `x-agent-org-id`, the
//! organisation the key was created in (auth-core checks it against the key's
//! own binding, so it is a claim Control verifies, never a scope this gateway
//! trusts). Per request this domain:
//! - forwards no caller header: the upstream request is built from an
//!   allow-list (the exchanged bearers, `Last-Event-ID` on the event stream and
//!   the request id), so a caller `Authorization`, any `x-*-authorization`,
//!   `x-org-id` or other scoping header never reaches model-gateway;
//! - exchanges the key at auth-core (`POST /api/{aud}/api-key-exchange`) with
//!   the gateway's own registered service credential, once per audience
//!   (`model-gateway`, `session-core`, and `inference-core` for a start), and
//!   caches each token under a digest of the key until `exp - 10 s`;
//! - forwards to model-gateway `/v1/agent/*` and answers with the codes of the
//!   S3 contract (`apps/Model Plane/api/contracts/agent-api-v1.json`) only;
//! - limits each key on its own bucket, before any exchange.
//!
//! The key goes to auth-core and nowhere else, and is never logged. A key whose
//! organisation is ZDR is refused by model-gateway (`zdr_feature_not_supported`);
//! this domain stores no content, only the short-lived tokens in memory.
//!
//! [`ApiKeyExchangeClient`] is the one exchange helper of this crate: INNH-10
//! GW-1 adds its browser audiences to [`ExchangeAudience`] rather than writing
//! a second client.
//!
//! Dark by default: unless `GATEWAY_AGENT_API_ENABLED` is the literal `true` at
//! startup, [`router`] adds no route and every path answers as before.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, RawQuery},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{config::AppState, envelope::error_with_details, rate_limit::RateLimiter};

/// The switch. Read once when the router is built, never in a handler.
pub(crate) const SWITCH_ENV: &str = "GATEWAY_AGENT_API_ENABLED";
/// Requests per key per minute; a non-positive or unparseable value keeps the default.
const RPM_PER_KEY_ENV: &str = "GATEWAY_AGENT_API_RPM_PER_KEY";
const DEFAULT_RPM_PER_KEY: f64 = 60.0;
/// The `verevon-gateway` registry principal's credential (its `consumerEnv` in
/// `apps/Control Plane/config/plane-service-principals.json`); the same
/// principal holds the `apiKeyExchange` audiences.
const CREDENTIAL_ENV: &str = "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY";
const SERVICE_ID_ENV: &str = "MAIL_TASK_GATEWAY_BRIDGE_SERVICE_ID";
const DEFAULT_SERVICE_ID: &str = "verevon-gateway";

const API_KEY_HEADER: &str = "x-api-key";
pub(crate) const ORG_HEADER: &str = "x-agent-org-id";
const LAST_EVENT_ID_HEADER: &str = "last-event-id";
/// The closed scope auth-core grants the three Agent API audiences.
const AGENT_SCOPE: &str = "agent:runs";
const GATEWAY_PREFIX: &str = "/v1/agent";

const MAX_API_KEY_CHARS: usize = 512;
const MAX_ID_CHARS: usize = 128;
const MAX_CURSOR_CHARS: usize = 512;
const MAX_LAST_EVENT_ID_CHARS: usize = 256;
const MAX_TOKEN_CHARS: usize = 8192;
const MAX_RELAYED_DETAIL_CHARS: usize = 256;
const PAGE_LIMIT_MAX: u32 = 100;
/// 32,000 characters of input at four bytes each, plus the other fields.
const MAX_START_BODY_BYTES: usize = 256 * 1024;
/// A cached token is dropped this long before its `exp`, so a token is never
/// forwarded in the last seconds of its life.
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(10);
/// Bound on the in-memory token cache; past it the cache is emptied rather than grown.
const TOKEN_CACHE_MAX_ENTRIES: usize = 10_000;
const DEFAULT_RETRY_AFTER_SECS: u64 = 60;
/// A closed error envelope is far smaller; a body past this is not one.
const UPSTREAM_ERROR_BODY_LIMIT: usize = 4096;

/// The strict switch idiom: only the literal `true` turns the ingress on.
pub(crate) fn switch_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

fn rpm_per_key(raw: Option<&str>) -> f64 {
    raw.and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_RPM_PER_KEY)
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let enabled = switch_enabled(std::env::var(SWITCH_ENV).ok().as_deref());
    let credential = if enabled {
        ServiceCredential::from_env()
    } else {
        None
    };
    if enabled && credential.is_none() {
        tracing::warn!(
            "agent api ingress is on but the gateway service credential is not set; every call answers agent_api_disabled"
        );
    }
    router_with(
        &state,
        enabled,
        credential,
        rpm_per_key(std::env::var(RPM_PER_KEY_ENV).ok().as_deref()),
    )
}

fn router_with(
    state: &AppState,
    enabled: bool,
    credential: Option<ServiceCredential>,
    rpm_per_key: f64,
) -> Router<AppState> {
    if !enabled {
        return Router::new();
    }
    routes(Arc::new(AgentApi::new(state, credential, rpm_per_key)))
}

fn routes(api: Arc<AgentApi>) -> Router<AppState> {
    Router::new()
        .route("/api/agent/v1/runs", post(start_run))
        .route("/api/agent/v1/runs/{run_id}", get(get_run))
        .route("/api/agent/v1/runs/{run_id}/events", get(run_events))
        .route("/api/agent/v1/runs/{run_id}/cancel", post(cancel_run))
        .route(
            "/api/agent/v1/threads/{thread_id}/runs",
            get(list_thread_runs),
        )
        .route(
            "/api/agent/v1/threads/{thread_id}/messages",
            get(list_thread_messages),
        )
        .layer(Extension(api))
}

// ---------------------------------------------------------------------------
// The S3 error contract
// ---------------------------------------------------------------------------

/// One code of the S3 golden: its status, the fixed public message this
/// gateway answers with, and the only `details` keys it may carry.
pub(crate) struct ErrorCode {
    pub(crate) code: &'static str,
    status: u16,
    message: &'static str,
    details_keys: &'static [&'static str],
}

static ERROR_CODES: [ErrorCode; 13] = [
    ErrorCode {
        code: "invalid_request",
        status: 400,
        message: "The request is not valid.",
        details_keys: &["field"],
    },
    ErrorCode {
        code: "api_key_invalid",
        status: 401,
        message: "The API key is missing, unknown, disabled or expired.",
        details_keys: &[],
    },
    ErrorCode {
        code: "agent_api_scope_missing",
        status: 403,
        message: "The API key does not hold the permission this route needs.",
        details_keys: &["scope"],
    },
    ErrorCode {
        code: "model_not_entitled",
        status: 403,
        message: "The requested model is not allowed for this key.",
        details_keys: &["model", "reason"],
    },
    ErrorCode {
        code: "run_not_found",
        status: 404,
        message: "The run was not found.",
        details_keys: &[],
    },
    ErrorCode {
        code: "thread_not_found",
        status: 404,
        message: "The thread was not found.",
        details_keys: &[],
    },
    ErrorCode {
        code: "zdr_feature_not_supported",
        status: 412,
        message: "The Agent API is not offered to an organisation with zero data retention.",
        details_keys: &["feature"],
    },
    ErrorCode {
        code: "agent_api_feature_not_supported",
        status: 422,
        message: "Agent API version 1 does not offer this feature.",
        details_keys: &["feature"],
    },
    ErrorCode {
        code: "rate_limited",
        status: 429,
        message: "Too many requests. Try again shortly.",
        details_keys: &["retry_after_seconds"],
    },
    ErrorCode {
        code: "internal_error",
        status: 500,
        message: "The request could not be completed.",
        details_keys: &[],
    },
    ErrorCode {
        code: "agent_api_disabled",
        status: 503,
        message: "The Agent API is not available.",
        details_keys: &[],
    },
    ErrorCode {
        code: "upstream_unavailable",
        status: 503,
        message: "The service is temporarily unavailable. Try again shortly.",
        details_keys: &[],
    },
    ErrorCode {
        code: "model_entitlement_unavailable",
        status: 503,
        message: "The model entitlement could not be checked. Try again shortly.",
        details_keys: &[],
    },
];

pub(crate) fn lookup_code(code: &str) -> Option<&'static ErrorCode> {
    ERROR_CODES.iter().find(|entry| entry.code == code)
}

/// The fallback entry; a test pins its position in [`ERROR_CODES`].
fn internal_error_code() -> &'static ErrorCode {
    &ERROR_CODES[9]
}

/// The typed envelope for `entry`. `details` is always an object; a
/// `rate_limited` answer also carries the wait in `Retry-After`.
fn error_response(entry: &'static ErrorCode, details: Value) -> Response {
    let details = match details {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    };
    let retry_after = details
        .get("retry_after_seconds")
        .and_then(Value::as_u64)
        .filter(|_| entry.code == "rate_limited");
    let status = StatusCode::from_u16(entry.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = Json(error_with_details(entry.code, entry.message, details));
    match retry_after {
        Some(seconds) => (status, [("retry-after", seconds.to_string())], body).into_response(),
        None => (status, body).into_response(),
    }
}

fn agent_error(code: &str, details: Value) -> Response {
    error_response(lookup_code(code).unwrap_or_else(internal_error_code), details)
}

fn invalid_request(field: &str) -> Response {
    agent_error("invalid_request", json!({ "field": field }))
}

fn rate_limited(retry_after_seconds: u64) -> Response {
    agent_error(
        "rate_limited",
        json!({ "retry_after_seconds": retry_after_seconds.max(1) }),
    )
}

/// Keep only the contract's `details` keys for `entry`, and only scalar values,
/// so an upstream detail (an internal id, a URL, a list) never reaches the caller.
fn relay_details(entry: &ErrorCode, upstream: Option<&Value>) -> Value {
    let mut out = Map::new();
    if let Some(Value::Object(details)) = upstream {
        for key in entry.details_keys {
            match details.get(*key) {
                Some(Value::String(value)) if value.chars().count() <= MAX_RELAYED_DETAIL_CHARS => {
                    out.insert((*key).to_owned(), Value::String(value.clone()));
                }
                Some(value @ (Value::Number(_) | Value::Bool(_))) => {
                    out.insert((*key).to_owned(), value.clone());
                }
                _ => {}
            }
        }
    }
    Value::Object(out)
}

/// Map a model-gateway refusal onto the S3 contract. A body that already names
/// a contract code is relayed with that code's fixed message and allowed
/// details; anything else is mapped by status. A 404 with no error code at all
/// is model-gateway without its Agent API routes (its switch off), not a
/// missing run.
pub(crate) fn upstream_refusal(
    status: u16,
    retry_after: Option<u64>,
    body: &Value,
    not_found: &'static str,
) -> (&'static ErrorCode, Value) {
    let upstream_code = body.pointer("/error/code").and_then(Value::as_str);
    if let Some(entry) = upstream_code.and_then(lookup_code) {
        let mut details = relay_details(entry, body.pointer("/error/details"));
        if entry.code == "rate_limited" && details.get("retry_after_seconds").is_none() {
            details["retry_after_seconds"] =
                json!(retry_after.unwrap_or(DEFAULT_RETRY_AFTER_SECS).max(1));
        }
        return (entry, details);
    }
    let (code, details) = match status {
        400 | 422 => ("invalid_request", json!({})),
        401 => ("api_key_invalid", json!({})),
        403 => ("agent_api_scope_missing", json!({ "scope": AGENT_SCOPE })),
        404 if upstream_code.is_none() => ("agent_api_disabled", json!({})),
        404 => (not_found, json!({})),
        412 => (
            "zdr_feature_not_supported",
            json!({ "feature": "agent_api" }),
        ),
        429 => (
            "rate_limited",
            json!({ "retry_after_seconds": retry_after.unwrap_or(DEFAULT_RETRY_AFTER_SECS).max(1) }),
        ),
        500..=599 => ("upstream_unavailable", json!({})),
        _ => ("internal_error", json!({})),
    };
    match lookup_code(code) {
        Some(entry) => (entry, details),
        None => (internal_error_code(), json!({})),
    }
}

fn retry_after_header(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

async fn read_error_body(response: reqwest::Response) -> Value {
    response
        .bytes()
        .await
        .ok()
        .filter(|bytes| bytes.len() <= UPSTREAM_ERROR_BODY_LIMIT)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// The API-key exchange client (shared with INNH-10 GW-1)
// ---------------------------------------------------------------------------

/// The gateway's own registered service credential. Deliberately not `Debug`,
/// so it cannot reach a log through a derived formatter.
#[derive(Clone)]
pub(crate) struct ServiceCredential {
    service_id: String,
    api_key: String,
}

impl ServiceCredential {
    fn from_env() -> Option<Self> {
        let api_key = std::env::var(CREDENTIAL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let service_id = std::env::var(SERVICE_ID_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SERVICE_ID.to_owned());
        Some(Self {
            service_id,
            api_key,
        })
    }
}

/// The audiences this gateway exchanges a key for: a closed subset of
/// auth-core's `API_KEY_EXCHANGE_AUDIENCE_SCOPES`, each with its one scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExchangeAudience {
    ModelGateway,
    SessionCore,
    InferenceCore,
}

impl ExchangeAudience {
    pub(crate) const fn claim(self) -> &'static str {
        match self {
            Self::ModelGateway => "model-gateway",
            Self::SessionCore => "session-core",
            Self::InferenceCore => "inference-core",
        }
    }

    pub(crate) const fn scope(self) -> &'static str {
        match self {
            Self::ModelGateway | Self::SessionCore | Self::InferenceCore => AGENT_SCOPE,
        }
    }
}

/// A one-way, domain-separated digest of an API key: the only form of the key
/// this gateway keeps (cache and rate-limit keys). Unsalted so every replica
/// lands on the same distributed rate-limit bucket; a key is a high-entropy
/// secret, so its digest does not let anyone recover it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeyDigest(String);

impl KeyDigest {
    pub(crate) fn of(api_key: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"verevon-gateway/agent-api-key/v1\0");
        hasher.update(api_key.as_bytes());
        Self(hex::encode(hasher.finalize()))
    }
}

/// The verified shape of one caller. The raw key lives here only for the
/// length of the request, to be sent to auth-core.
pub(crate) struct KeyHolder {
    api_key: String,
    org_id: String,
    digest: KeyDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExchangeError {
    /// The gateway has no service credential: the ingress cannot exchange.
    NotConfigured,
    KeyInvalid,
    ScopeMissing,
    /// auth-core does not offer the exchange to this gateway or audience
    /// (switch off, registry entry absent).
    Disabled,
    RateLimited(u64),
    Unavailable,
    Internal,
}

impl ExchangeError {
    fn into_response(self) -> Response {
        match self {
            Self::NotConfigured | Self::Disabled => agent_error("agent_api_disabled", json!({})),
            Self::KeyInvalid => agent_error("api_key_invalid", json!({})),
            Self::ScopeMissing => agent_error("agent_api_scope_missing", json!({ "scope": AGENT_SCOPE })),
            Self::RateLimited(seconds) => rate_limited(seconds),
            Self::Unavailable => agent_error("upstream_unavailable", json!({})),
            Self::Internal => agent_error("internal_error", json!({})),
        }
    }
}

/// Map an auth-core refusal (`{ error: { code, message, details: { reason } } }`)
/// onto what the caller may learn. An organisation key, a key bound to another
/// organisation and an owner who left it all read as an invalid key; a caller or
/// audience auth-core does not serve reads as the API being off; a malformed
/// request is this gateway's fault, never the caller's.
pub(crate) fn classify_exchange_refusal(
    status: u16,
    reason: Option<&str>,
    retry_after: Option<u64>,
) -> ExchangeError {
    match status {
        401 => ExchangeError::KeyInvalid,
        403 => match reason {
            Some("scope_not_granted") => ExchangeError::ScopeMissing,
            Some("org_key_refused" | "org_mismatch" | "owner_not_member") => {
                ExchangeError::KeyInvalid
            }
            _ => ExchangeError::Disabled,
        },
        404 => ExchangeError::Disabled,
        429 => ExchangeError::RateLimited(retry_after.unwrap_or(DEFAULT_RETRY_AFTER_SECS).max(1)),
        400 => ExchangeError::Internal,
        _ => ExchangeError::Unavailable,
    }
}

#[derive(Deserialize)]
struct ExchangeEnvelope {
    data: ExchangeGrant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeGrant {
    token: String,
    expires_at: String,
    audience: String,
}

struct CachedToken {
    token: String,
    fresh_until: Instant,
}

type CacheKey = (KeyDigest, String, ExchangeAudience);

/// When a token that expires at `expires_at` stops being served from cache:
/// `exp - 10 s`, or `None` (do not cache) when that is already past or the
/// expiry cannot be read.
pub(crate) fn cache_deadline(expires_at: &str, now_utc: DateTime<Utc>, now: Instant) -> Option<Instant> {
    let expires = DateTime::parse_from_rfc3339(expires_at).ok()?.with_timezone(&Utc);
    let remaining = (expires - now_utc).to_std().ok()?;
    let usable = remaining.checked_sub(TOKEN_EXPIRY_MARGIN)?;
    if usable.is_zero() {
        return None;
    }
    Some(now + usable)
}

fn is_bearer_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_CHARS
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(crate) struct ApiKeyExchangeClient {
    auth_core_url: String,
    http: reqwest::Client,
    credential: Option<ServiceCredential>,
    cache: Mutex<HashMap<CacheKey, CachedToken>>,
}

impl ApiKeyExchangeClient {
    pub(crate) fn new(
        auth_core_url: &str,
        http: reqwest::Client,
        credential: Option<ServiceCredential>,
    ) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            http,
            credential,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.credential.is_some()
    }

    fn cached(&self, key: &CacheKey) -> Option<String> {
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache
            .get(key)
            .filter(|entry| entry.fresh_until > Instant::now())
            .map(|entry| entry.token.clone())
    }

    fn remember(&self, key: CacheKey, token: String, fresh_until: Instant) {
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        cache.retain(|_, entry| entry.fresh_until > now);
        if cache.len() >= TOKEN_CACHE_MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(key, CachedToken { token, fresh_until });
    }

    /// Drop every token held for this key, for example after an upstream
    /// answered 401 to one (the key was revoked inside a token's lifetime).
    pub(crate) fn forget(&self, digest: &KeyDigest) {
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|(cached, _, _), _| cached != digest);
    }

    /// One `aud`-bound `api_key` token for `holder`, from cache or from
    /// auth-core. The key is sent in the body of this one request and nowhere else.
    pub(crate) async fn token(
        &self,
        holder: &KeyHolder,
        audience: ExchangeAudience,
    ) -> Result<String, ExchangeError> {
        let Some(credential) = self.credential.as_ref() else {
            return Err(ExchangeError::NotConfigured);
        };
        let cache_key = (holder.digest.clone(), holder.org_id.clone(), audience);
        if let Some(token) = self.cached(&cache_key) {
            return Ok(token);
        }

        let request = self
            .http
            .post(format!(
                "{}/api/{}/api-key-exchange",
                self.auth_core_url,
                audience.claim()
            ))
            .header("x-service-id", &credential.service_id)
            .header("x-service-api-key", &credential.api_key)
            .json(&json!({
                "apiKey": holder.api_key,
                "orgId": holder.org_id,
                "scopes": [audience.scope()],
            }));
        let response = crate::request_id::apply(request)
            .send()
            .await
            .map_err(|transport| {
                tracing::warn!(
                    error_class = crate::zdr_log::class_reqwest(&transport),
                    audience = audience.claim(),
                    "agent api key exchange did not reach auth-core"
                );
                ExchangeError::Unavailable
            })?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = retry_after_header(response.headers());
            let body = read_error_body(response).await;
            let reason = body.pointer("/error/details/reason").and_then(Value::as_str);
            return Err(classify_exchange_refusal(status.as_u16(), reason, retry_after));
        }
        let grant = response
            .json::<ExchangeEnvelope>()
            .await
            .map_err(|_| ExchangeError::Unavailable)?
            .data;
        if grant.audience != audience.claim() || !is_bearer_token(&grant.token) {
            return Err(ExchangeError::Unavailable);
        }
        if let Some(fresh_until) = cache_deadline(&grant.expires_at, Utc::now(), Instant::now()) {
            self.remember(cache_key, grant.token.clone(), fresh_until);
        }
        Ok(grant.token)
    }
}

// ---------------------------------------------------------------------------
// The ingress
// ---------------------------------------------------------------------------

pub(crate) struct AgentApi {
    exchange: ApiKeyExchangeClient,
    model_gateway_url: String,
    http: reqwest::Client,
    streaming_http: reqwest::Client,
    limiter: RateLimiter,
}

/// The exchanged bearers of one request: the only authority forwarded upstream.
struct Bearers {
    model_gateway: String,
    session: String,
    inference: Option<String>,
}

impl Bearers {
    fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut request = request
            .bearer_auth(&self.model_gateway)
            .header("x-session-authorization", format!("Bearer {}", self.session));
        if let Some(inference) = &self.inference {
            request = request.header("x-inference-authorization", format!("Bearer {inference}"));
        }
        crate::request_id::apply(request)
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    first.to_str().ok()
}

fn is_api_key_shaped(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_API_KEY_CHARS
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Ids are opaque tokens of letters, digits, `-` and `_`: nothing that could
/// add a path segment or a query to the upstream URL.
pub(crate) fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn validate_id(value: &str, field: &'static str) -> Result<(), Response> {
    if is_identifier(value) {
        Ok(())
    } else {
        Err(invalid_request(field))
    }
}

/// Rebuild the query of a paged read from its closed allow-list (`cursor_field`
/// and `limit`, each at most once). Anything else, the key included, is refused
/// rather than forwarded; the refusal names `query`, never the caller's text.
pub(crate) fn page_query(raw: Option<&str>, cursor_field: &'static str) -> Result<String, Response> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    let mut seen_cursor = false;
    let mut seen_limit = false;
    for (name, value) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        if name == cursor_field && !seen_cursor {
            if value.is_empty()
                || value.len() > MAX_CURSOR_CHARS
                || !value.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(invalid_request(cursor_field));
            }
            seen_cursor = true;
            serializer.append_pair(cursor_field, &value);
        } else if name == "limit" && !seen_limit {
            let limit = match value.parse::<u32>() {
                Ok(limit) if (1..=PAGE_LIMIT_MAX).contains(&limit) => limit,
                _ => return Err(invalid_request("limit")),
            };
            seen_limit = true;
            serializer.append_pair("limit", &limit.to_string());
        } else {
            return Err(invalid_request("query"));
        }
    }
    Ok(serializer.finish())
}

async fn read_start_body(body: Body) -> Result<Value, Response> {
    let bytes: Bytes = axum::body::to_bytes(body, MAX_START_BODY_BYTES)
        .await
        .map_err(|_| invalid_request("body"))?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value @ Value::Object(_)) => Ok(value),
        _ => Err(invalid_request("body")),
    }
}

fn read_last_event_id(headers: &HeaderMap) -> Result<Option<String>, Response> {
    if !headers.contains_key(LAST_EVENT_ID_HEADER) {
        return Ok(None);
    }
    match single_header(headers, LAST_EVENT_ID_HEADER) {
        Some(value)
            if !value.is_empty()
                && value.len() <= MAX_LAST_EVENT_ID_CHARS
                && value.bytes().all(|byte| byte.is_ascii_graphic()) =>
        {
            Ok(Some(value.to_owned()))
        }
        _ => Err(invalid_request("Last-Event-ID")),
    }
}

fn sse_transport_error_event() -> Bytes {
    Bytes::from_static(
        b"event: error\ndata: {\"error\":{\"code\":\"upstream_unavailable\",\"message\":\"The service is temporarily unavailable. Try again shortly.\",\"details\":{}}}\n\n",
    )
}

impl AgentApi {
    fn new(state: &AppState, credential: Option<ServiceCredential>, rpm_per_key: f64) -> Self {
        Self {
            exchange: ApiKeyExchangeClient::new(&state.auth_core_url, state.client.clone(), credential),
            model_gateway_url: state.model_gateway_url.trim_end_matches('/').to_owned(),
            http: state.client.clone(),
            streaming_http: state.streaming_client.clone(),
            limiter: RateLimiter::from_cache_with_rpm(&state.cache, rpm_per_key),
        }
    }

    /// Who is calling: a well-formed key, the organisation it claims, and a
    /// token from that key's own bucket. Runs before any exchange, so a flood of
    /// one key never reaches auth-core.
    async fn admit(&self, headers: &HeaderMap) -> Result<KeyHolder, Response> {
        if !self.exchange.is_configured() {
            return Err(agent_error("agent_api_disabled", json!({})));
        }
        let api_key = single_header(headers, API_KEY_HEADER)
            .filter(|value| is_api_key_shaped(value))
            .ok_or_else(|| agent_error("api_key_invalid", json!({})))?;
        let org_id = single_header(headers, ORG_HEADER)
            .filter(|value| is_identifier(value))
            .ok_or_else(|| invalid_request(ORG_HEADER))?;
        let digest = KeyDigest::of(api_key);
        if let Err(retry_after) = self
            .limiter
            .try_acquire(&format!("agent_api:{}", digest.0))
            .await
        {
            return Err(rate_limited(retry_after));
        }
        Ok(KeyHolder {
            api_key: api_key.to_owned(),
            org_id: org_id.to_owned(),
            digest,
        })
    }

    async fn bearers(&self, holder: &KeyHolder, start: bool) -> Result<Bearers, Response> {
        let (model_gateway, session) = tokio::try_join!(
            self.exchange.token(holder, ExchangeAudience::ModelGateway),
            self.exchange.token(holder, ExchangeAudience::SessionCore),
        )
        .map_err(ExchangeError::into_response)?;
        let inference = if start {
            Some(
                self.exchange
                    .token(holder, ExchangeAudience::InferenceCore)
                    .await
                    .map_err(ExchangeError::into_response)?,
            )
        } else {
            None
        };
        Ok(Bearers {
            model_gateway,
            session,
            inference,
        })
    }

    fn refusal(
        &self,
        holder: &KeyHolder,
        status: u16,
        retry_after: Option<u64>,
        body: &Value,
        not_found: &'static str,
    ) -> Response {
        let (entry, details) = upstream_refusal(status, retry_after, body, not_found);
        if entry.code == "api_key_invalid" {
            self.exchange.forget(&holder.digest);
        }
        error_response(entry, details)
    }

    fn transport_failure(transport: &reqwest::Error) -> Response {
        tracing::warn!(
            error_class = crate::zdr_log::class_reqwest(transport),
            "agent api request did not reach model-gateway"
        );
        agent_error("upstream_unavailable", json!({}))
    }

    #[allow(clippy::too_many_arguments)] // one forwarded request, every part named
    async fn forward_json(
        &self,
        holder: &KeyHolder,
        method: Method,
        path: &str,
        query: &str,
        body: Option<Value>,
        bearers: &Bearers,
        not_found: &'static str,
    ) -> Response {
        let mut target = format!("{}{GATEWAY_PREFIX}{path}", self.model_gateway_url);
        if !query.is_empty() {
            target.push('?');
            target.push_str(query);
        }
        let mut request = bearers.apply(self.http.request(method, &target));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(transport) => return Self::transport_failure(&transport),
        };
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let retry_after = retry_after_header(response.headers());
            let body = read_error_body(response).await;
            return self.refusal(holder, status, retry_after, &body, not_found);
        }
        match response.json::<Value>().await {
            Ok(value) => (
                StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                Json(value),
            )
                .into_response(),
            Err(_) => agent_error("upstream_unavailable", json!({})),
        }
    }

    async fn forward_events(
        &self,
        holder: &KeyHolder,
        run_id: &str,
        last_event_id: Option<&str>,
        bearers: &Bearers,
    ) -> Response {
        let target = format!("{}{GATEWAY_PREFIX}/runs/{run_id}/events", self.model_gateway_url);
        let mut request = bearers
            .apply(self.streaming_http.get(&target))
            .header("accept", "text/event-stream");
        if let Some(last_event_id) = last_event_id {
            request = request.header(LAST_EVENT_ID_HEADER, last_event_id);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(transport) => return Self::transport_failure(&transport),
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let retry_after = retry_after_header(response.headers());
            let body = read_error_body(response).await;
            return self.refusal(holder, status, retry_after, &body, "run_not_found");
        }

        // Upstream event bytes verbatim, with a `: keep-alive` comment every 15 s
        // of idle so an intermediary does not reap the stream (as upstream.rs does
        // for the chat streams).
        let upstream = response.bytes_stream();
        let body = Body::from_stream(async_stream::stream! {
            futures_util::pin_mut!(upstream);
            let mut ticker = tokio::time::interval(Duration::from_secs(15));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    chunk = upstream.next() => match chunk {
                        Some(Ok(bytes)) => yield Ok::<Bytes, std::io::Error>(bytes),
                        Some(Err(_)) => {
                            yield Ok::<Bytes, std::io::Error>(sse_transport_error_event());
                            break;
                        }
                        None => break,
                    },
                    _ = ticker.tick() => {
                        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": keep-alive\n\n"));
                    }
                }
            }
        });
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(body)
            .unwrap_or_else(|_| agent_error("internal_error", json!({})))
    }
}

// ---------------------------------------------------------------------------
// Handlers: admit, validate, exchange, forward. Validation runs before the
// exchange so a malformed request never spends a key verification.
// ---------------------------------------------------------------------------

async fn start_run(
    Extension(api): Extension<Arc<AgentApi>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let holder = match api.admit(&headers).await {
        Ok(holder) => holder,
        Err(response) => return response,
    };
    let body = match read_start_body(body).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let bearers = match api.bearers(&holder, true).await {
        Ok(bearers) => bearers,
        Err(response) => return response,
    };
    api.forward_json(
        &holder,
        Method::POST,
        "/runs",
        "",
        Some(body),
        &bearers,
        "thread_not_found",
    )
    .await
}

async fn get_run(
    Extension(api): Extension<Arc<AgentApi>>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let holder = match api.admit(&headers).await {
        Ok(holder) => holder,
        Err(response) => return response,
    };
    if let Err(response) = validate_id(&run_id, "run_id") {
        return response;
    }
    let bearers = match api.bearers(&holder, false).await {
        Ok(bearers) => bearers,
        Err(response) => return response,
    };
    api.forward_json(
        &holder,
        Method::GET,
        &format!("/runs/{run_id}"),
        "",
        None,
        &bearers,
        "run_not_found",
    )
    .await
}

async fn run_events(
    Extension(api): Extension<Arc<AgentApi>>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let holder = match api.admit(&headers).await {
        Ok(holder) => holder,
        Err(response) => return response,
    };
    if let Err(response) = validate_id(&run_id, "run_id") {
        return response;
    }
    let last_event_id = match read_last_event_id(&headers) {
        Ok(last_event_id) => last_event_id,
        Err(response) => return response,
    };
    let bearers = match api.bearers(&holder, false).await {
        Ok(bearers) => bearers,
        Err(response) => return response,
    };
    api.forward_events(&holder, &run_id, last_event_id.as_deref(), &bearers)
        .await
}

async fn cancel_run(
    Extension(api): Extension<Arc<AgentApi>>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let holder = match api.admit(&headers).await {
        Ok(holder) => holder,
        Err(response) => return response,
    };
    if let Err(response) = validate_id(&run_id, "run_id") {
        return response;
    }
    let bearers = match api.bearers(&holder, false).await {
        Ok(bearers) => bearers,
        Err(response) => return response,
    };
    // The cancel route takes no body (S3 golden `body_fields: []`); a caller body is not read.
    api.forward_json(
        &holder,
        Method::POST,
        &format!("/runs/{run_id}/cancel"),
        "",
        None,
        &bearers,
        "run_not_found",
    )
    .await
}

async fn list_thread_runs(
    Extension(api): Extension<Arc<AgentApi>>,
    Path(thread_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    list_thread(api, thread_id, query, headers, "runs", "after").await
}

async fn list_thread_messages(
    Extension(api): Extension<Arc<AgentApi>>,
    Path(thread_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    list_thread(api, thread_id, query, headers, "messages", "before").await
}

async fn list_thread(
    api: Arc<AgentApi>,
    thread_id: String,
    query: Option<String>,
    headers: HeaderMap,
    collection: &'static str,
    cursor_field: &'static str,
) -> Response {
    let holder = match api.admit(&headers).await {
        Ok(holder) => holder,
        Err(response) => return response,
    };
    if let Err(response) = validate_id(&thread_id, "thread_id") {
        return response;
    }
    let query = match page_query(query.as_deref(), cursor_field) {
        Ok(query) => query,
        Err(response) => return response,
    };
    let bearers = match api.bearers(&holder, false).await {
        Ok(bearers) => bearers,
        Err(response) => return response,
    };
    api.forward_json(
        &holder,
        Method::GET,
        &format!("/threads/{thread_id}/{collection}"),
        &query,
        None,
        &bearers,
        "thread_not_found",
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::{
        cache_deadline, classify_exchange_refusal, internal_error_code, is_identifier,
        lookup_code, page_query, router_with, routes, switch_enabled, upstream_refusal, AgentApi,
        ExchangeError, KeyDigest, ServiceCredential, AGENT_SCOPE, ERROR_CODES, GATEWAY_PREFIX,
        ORG_HEADER, TOKEN_EXPIRY_MARGIN,
    };
    use crate::config::AppState;

    const KEY: &str = "agent-test-key-0123456789abcdef";
    const OTHER_KEY: &str = "agent-test-key-fedcba9876543210";
    const ORG: &str = "org-proof";
    const SERVICE_SECRET: &str = "gateway-test-credential";

    fn credential() -> ServiceCredential {
        ServiceCredential {
            service_id: "verevon-gateway".to_owned(),
            api_key: SERVICE_SECRET.to_owned(),
        }
    }

    fn state_for(auth: &MockServer, model_gateway: &MockServer) -> AppState {
        AppState {
            auth_core_url: auth.uri(),
            model_gateway_url: model_gateway.uri(),
            ..AppState::for_tests()
        }
    }

    fn api_for(auth: &MockServer, model_gateway: &MockServer, rpm: f64) -> Arc<AgentApi> {
        Arc::new(AgentApi::new(
            &state_for(auth, model_gateway),
            Some(credential()),
            rpm,
        ))
    }

    fn app(api: Arc<AgentApi>) -> Router {
        routes(api).with_state(AppState::for_tests())
    }

    fn expires_in(seconds: i64) -> String {
        (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339()
    }

    async fn mount_exchange(auth: &MockServer, audience: &str, token: &str, ttl: i64, calls: u64) {
        Mock::given(method("POST"))
            .and(path(format!("/api/{audience}/api-key-exchange")))
            .and(header("x-service-id", "verevon-gateway"))
            .and(header("x-service-api-key", SERVICE_SECRET))
            .and(body_partial_json(json!({
                "apiKey": KEY,
                "orgId": ORG,
                "scopes": ["agent:runs"],
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "data": { "token": token, "expiresAt": expires_in(ttl), "audience": audience }
            })))
            .expect(calls)
            .mount(auth)
            .await;
    }

    fn keyed(method: &str, uri: &str, key: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("x-api-key", key)
            .header(ORG_HEADER, ORG)
    }

    async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.expect("router answers");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// `{ error: { code, message, details } }` and nothing else, `details` an object.
    fn assert_envelope(body: &Value, code: &str) {
        assert_eq!(body.as_object().map(|object| object.len()), Some(1), "{body}");
        let error = body["error"].as_object().expect("error object");
        assert_eq!(error.len(), 3, "{body}");
        assert_eq!(error["code"], code, "{body}");
        assert!(error["message"].is_string());
        assert!(error["details"].is_object());
    }

    #[test]
    fn switch_takes_only_literal_true() {
        assert!(switch_enabled(Some("true")));
        for raw in [None, Some(""), Some("TRUE"), Some(" true"), Some("1"), Some("yes"), Some("on")] {
            assert!(!switch_enabled(raw), "{raw:?}");
        }
    }

    #[tokio::test]
    async fn switch_off_route_absent() {
        let state = AppState::for_tests();
        let off = router_with(&state, false, Some(credential()), 60.0).with_state(state.clone());
        let (status, _) = send(
            off,
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let on = router_with(&state, true, Some(credential()), 60.0).with_state(state);
        let (status, body) = send(
            on,
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_ne!(status, StatusCode::NOT_FOUND);
        assert_envelope(&body, "upstream_unavailable");
    }

    #[tokio::test]
    async fn switch_on_full_router_has_no_overlapping_routes() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let previous = std::env::var_os(super::SWITCH_ENV);
        std::env::set_var(super::SWITCH_ENV, "true");
        let built = std::panic::catch_unwind(|| {
            let _ = crate::build_router(AppState::for_tests());
        });
        match previous {
            Some(value) => std::env::set_var(super::SWITCH_ENV, value),
            None => std::env::remove_var(super::SWITCH_ENV),
        }
        assert!(built.is_ok(), "the Agent API routes overlap another domain's");
    }

    #[tokio::test]
    async fn without_a_service_credential_every_route_is_disabled() {
        let state = AppState::for_tests();
        let api = Arc::new(AgentApi::new(&state, None, 60.0));
        let (status, body) = send(
            app(api),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_envelope(&body, "agent_api_disabled");
    }

    #[tokio::test]
    async fn missing_or_repeated_key_and_missing_org_are_refused_before_exchange() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        let api = api_for(&auth, &model_gateway, 60.0);

        let (status, body) = send(
            app(api.clone()),
            Request::builder()
                .uri("/api/agent/v1/runs/run-1")
                .header(ORG_HEADER, ORG)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_envelope(&body, "api_key_invalid");

        let (status, body) = send(
            app(api.clone()),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .header("x-api-key", OTHER_KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_envelope(&body, "api_key_invalid");

        let (status, body) = send(
            app(api),
            Request::builder()
                .uri("/api/agent/v1/runs/run-1")
                .header("x-api-key", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_envelope(&body, "invalid_request");
        assert_eq!(body["error"]["details"]["field"], ORG_HEADER);

        assert!(auth.received_requests().await.expect("recorded").is_empty());
        assert!(model_gateway.received_requests().await.expect("recorded").is_empty());
    }

    #[tokio::test]
    async fn key_is_only_sent_to_auth_core() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "run": { "run_id": "run-1", "status": "completed" } }
            })))
            .expect(1)
            .mount(&model_gateway)
            .await;

        let (status, body) = send(
            app(api_for(&auth, &model_gateway, 60.0)),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["run"]["run_id"], "run-1");

        for forwarded in model_gateway.received_requests().await.expect("recorded") {
            assert!(forwarded.headers.get("x-api-key").is_none());
            for (_, value) in forwarded.headers.iter() {
                assert!(!value.to_str().unwrap_or_default().contains(KEY));
            }
            assert!(!String::from_utf8_lossy(&forwarded.body).contains(KEY));
            assert!(!forwarded.url.as_str().contains(KEY));
        }
    }

    #[tokio::test]
    async fn caller_authorization_headers_are_stripped() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .and(header("authorization", "Bearer mg-token"))
            .and(header("x-session-authorization", "Bearer session-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "run": {} } })))
            .expect(1)
            .mount(&model_gateway)
            .await;

        let (status, _) = send(
            app(api_for(&auth, &model_gateway, 60.0)),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .header("authorization", "Bearer forged-user-token")
                .header("x-session-authorization", "Bearer forged-session")
                .header("x-inference-authorization", "Bearer forged-inference")
                .header("x-browser-authorization", "Bearer forged-browser")
                .header("x-org-id", "org-other")
                .header("x-verevon-org-id", "org-other")
                .header("x-user-id", "user-forged")
                .header("cookie", "better-auth.session=forged")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let forwarded = model_gateway.received_requests().await.expect("recorded");
        assert_eq!(forwarded.len(), 1);
        let headers = &forwarded[0].headers;
        assert_eq!(headers.get("authorization").unwrap(), "Bearer mg-token");
        assert_eq!(headers.get("x-session-authorization").unwrap(), "Bearer session-token");
        for absent in [
            "x-inference-authorization",
            "x-browser-authorization",
            "x-org-id",
            "x-verevon-org-id",
            "x-user-id",
            "cookie",
            ORG_HEADER,
        ] {
            assert!(headers.get(absent).is_none(), "{absent} was forwarded");
        }
    }

    #[tokio::test]
    async fn start_route_also_exchanges_the_inference_token() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        mount_exchange(&auth, "inference-core", "inference-token", 120, 1).await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .and(header("x-inference-authorization", "Bearer inference-token"))
            .and(body_partial_json(json!({ "input": "Summarise the report" })))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "data": { "run_id": "run-9", "thread_id": "thread-9", "status": "queued" }
            })))
            .expect(1)
            .mount(&model_gateway)
            .await;

        let (status, body) = send(
            app(api_for(&auth, &model_gateway, 60.0)),
            keyed("POST", "/api/agent/v1/runs", KEY)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"input":"Summarise the report"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body["data"]["status"], "queued");
    }

    #[tokio::test]
    async fn start_body_must_be_a_json_object() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        for raw in ["not json", "[1,2]", "\"input\""] {
            let (status, body) = send(
                app(api_for(&auth, &model_gateway, 60.0)),
                keyed("POST", "/api/agent/v1/runs", KEY)
                    .body(Body::from(raw))
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_envelope(&body, "invalid_request");
            assert_eq!(body["error"]["details"]["field"], "body");
        }
        assert!(auth.received_requests().await.expect("recorded").is_empty());
    }

    #[test]
    fn exchange_refusals_classify_into_the_contract() {
        assert_eq!(classify_exchange_refusal(401, Some("key_disabled"), None), ExchangeError::KeyInvalid);
        assert_eq!(
            classify_exchange_refusal(403, Some("scope_not_granted"), None),
            ExchangeError::ScopeMissing
        );
        for reason in ["org_key_refused", "org_mismatch", "owner_not_member"] {
            assert_eq!(classify_exchange_refusal(403, Some(reason), None), ExchangeError::KeyInvalid);
        }
        assert_eq!(
            classify_exchange_refusal(403, Some("caller_not_authorized"), None),
            ExchangeError::Disabled
        );
        assert_eq!(classify_exchange_refusal(404, None, None), ExchangeError::Disabled);
        assert_eq!(classify_exchange_refusal(429, None, Some(7)), ExchangeError::RateLimited(7));
        assert_eq!(classify_exchange_refusal(429, None, Some(0)), ExchangeError::RateLimited(1));
        assert_eq!(classify_exchange_refusal(400, None, None), ExchangeError::Internal);
        assert_eq!(classify_exchange_refusal(503, None, None), ExchangeError::Unavailable);
    }

    #[tokio::test]
    async fn exchange_denial_maps_to_typed_error() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-gateway/api-key-exchange"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {
                    "code": "api_key_exchange_denied",
                    "message": "The API key cannot be exchanged for this audience",
                    "details": { "reason": "scope_not_granted" }
                }
            })))
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/session-core/api-key-exchange"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": { "code": "api_key_exchange_denied", "details": { "reason": "scope_not_granted" } }
            })))
            .mount(&auth)
            .await;

        let (status, body) = send(
            app(api_for(&auth, &model_gateway, 60.0)),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_envelope(&body, "agent_api_scope_missing");
        assert_eq!(body["error"]["details"], json!({ "scope": "agent:runs" }));
        assert!(model_gateway.received_requests().await.expect("recorded").is_empty());
    }

    #[test]
    fn token_cache_deadline_is_exp_minus_margin() {
        let now_utc = chrono::Utc::now();
        let now = std::time::Instant::now();
        let at = |seconds: i64| (now_utc + chrono::Duration::seconds(seconds)).to_rfc3339();

        let deadline = cache_deadline(&at(120), now_utc, now).expect("cached");
        let usable = deadline.duration_since(now);
        assert!(usable <= std::time::Duration::from_secs(120) - TOKEN_EXPIRY_MARGIN);
        assert!(usable >= std::time::Duration::from_secs(109));

        assert!(cache_deadline(&at(10), now_utc, now).is_none());
        assert!(cache_deadline(&at(5), now_utc, now).is_none());
        assert!(cache_deadline(&at(-30), now_utc, now).is_none());
        assert!(cache_deadline("not-a-date", now_utc, now).is_none());
    }

    #[tokio::test]
    async fn token_cache_is_keyed_by_hash_and_expires() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        // One exchange per audience serves both requests.
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "run": {} } })))
            .expect(2)
            .mount(&model_gateway)
            .await;

        let api = api_for(&auth, &model_gateway, 60.0);
        for _ in 0..2 {
            let (status, _) = send(
                app(api.clone()),
                keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        {
            let cache = api.exchange.cache.lock().unwrap();
            assert_eq!(cache.len(), 2);
            for (digest, org, _) in cache.keys() {
                assert_eq!(digest, &KeyDigest::of(KEY));
                assert!(!digest.0.contains(KEY));
                assert_eq!(org, ORG);
            }
        }

        // A token inside the expiry margin is never cached: each request exchanges again.
        let short_auth = MockServer::start().await;
        mount_exchange(&short_auth, "model-gateway", "mg-token", 5, 2).await;
        mount_exchange(&short_auth, "session-core", "session-token", 5, 2).await;
        let short_gateway = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "run": {} } })))
            .expect(2)
            .mount(&short_gateway)
            .await;
        let short = api_for(&short_auth, &short_gateway, 60.0);
        for _ in 0..2 {
            let _ = send(
                app(short.clone()),
                keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        }
        assert!(short.exchange.cache.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn upstream_401_forgets_the_cached_tokens() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&model_gateway)
            .await;

        let api = api_for(&auth, &model_gateway, 60.0);
        let (status, body) = send(
            app(api.clone()),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_envelope(&body, "api_key_invalid");
        assert!(api.exchange.cache.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn rate_limit_per_key() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "run": {} } })))
            .mount(&model_gateway)
            .await;

        // One request per minute per key.
        let api = api_for(&auth, &model_gateway, 1.0);
        let (first, _) = send(
            app(api.clone()),
            keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(first, StatusCode::OK);

        let response = app(api.clone())
            .oneshot(
                keyed("GET", "/api/agent/v1/runs/run-1", KEY)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router answers");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().get("retry-after").is_some());
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_envelope(&body, "rate_limited");
        assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap_or(0) >= 1);

        // Another key has its own bucket (its exchange is refused by the mock, not limited).
        let (other, _) = send(
            app(api),
            keyed("GET", "/api/agent/v1/runs/run-1", OTHER_KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_ne!(other, StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn upstream_errors_relay_only_contract_codes_and_details() {
        let zdr = json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "session-core said org org-1 is zdr",
                "details": { "feature": "agent_api", "org_id": "org-1" }
            }
        });
        let (entry, details) = upstream_refusal(412, None, &zdr, "run_not_found");
        assert_eq!(entry.code, "zdr_feature_not_supported");
        assert_eq!(details, json!({ "feature": "agent_api" }));

        let unknown = json!({ "error": { "code": "session_core_unavailable", "message": "x" } });
        assert_eq!(upstream_refusal(503, None, &unknown, "run_not_found").0.code, "upstream_unavailable");
        assert_eq!(upstream_refusal(404, None, &unknown, "thread_not_found").0.code, "thread_not_found");
        assert_eq!(upstream_refusal(404, None, &Value::Null, "run_not_found").0.code, "agent_api_disabled");
        assert_eq!(upstream_refusal(401, None, &Value::Null, "run_not_found").0.code, "api_key_invalid");

        let (entry, details) = upstream_refusal(429, Some(7), &Value::Null, "run_not_found");
        assert_eq!(entry.code, "rate_limited");
        assert_eq!(details, json!({ "retry_after_seconds": 7 }));

        let listed = json!({ "error": { "code": "agent_api_feature_not_supported", "details": ["tools"] } });
        let (entry, details) = upstream_refusal(422, None, &listed, "run_not_found");
        assert_eq!(entry.code, "agent_api_feature_not_supported");
        assert_eq!(details, json!({}));
    }

    #[tokio::test]
    async fn upstream_refusal_reaches_the_caller_as_a_typed_envelope() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run-1/cancel"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "run_not_found", "message": "no row in runs for org-1", "details": {} }
            })))
            .mount(&model_gateway)
            .await;

        let (status, body) = send(
            app(api_for(&auth, &model_gateway, 60.0)),
            keyed("POST", "/api/agent/v1/runs/run-1/cancel", KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_envelope(&body, "run_not_found");
        assert_eq!(body["error"]["message"], "The run was not found.");
    }

    #[tokio::test]
    async fn events_stream_forwards_last_event_id_and_bytes() {
        let auth = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        mount_exchange(&auth, "model-gateway", "mg-token", 120, 1).await;
        mount_exchange(&auth, "session-core", "session-token", 120, 1).await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-1/events"))
            .and(header("last-event-id", "42"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("event: run.completed\ndata: {}\n\n"),
            )
            .expect(1)
            .mount(&model_gateway)
            .await;

        let response = app(api_for(&auth, &model_gateway, 60.0))
            .oneshot(
                keyed("GET", "/api/agent/v1/runs/run-1/events", KEY)
                    .header("last-event-id", "42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router answers");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains("event: run.completed"));
    }

    #[test]
    fn error_codes_match_the_s3_golden() {
        assert_eq!(internal_error_code().code, "internal_error");
        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../Model Plane/api/contracts/agent-api-v1.json");
        let Ok(raw) = std::fs::read_to_string(&golden_path) else {
            // A build context without the Model Plane tree (the gateway image) has no golden.
            return;
        };
        let golden: Value = serde_json::from_str(&raw).expect("golden parses");
        let codes = golden["error_codes"].as_array().expect("error_codes");
        assert_eq!(codes.len(), ERROR_CODES.len());
        for code in codes {
            let name = code["code"].as_str().expect("code");
            let entry = lookup_code(name).unwrap_or_else(|| panic!("{name} is not in ERROR_CODES"));
            assert_eq!(u64::from(entry.status), code["http_status"].as_u64().expect("status"));
            let keys: Vec<&str> = code["details_keys"]
                .as_array()
                .expect("details_keys")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(entry.details_keys, keys.as_slice(), "{name}");
        }
        assert_eq!(golden["auth"]["header"], "x-api-key");
        assert_eq!(golden["auth"]["scope"], AGENT_SCOPE);
        assert_eq!(golden["spec"]["public_prefix"], "/api/agent/v1");
        assert_eq!(golden["spec"]["gateway_prefix"], GATEWAY_PREFIX);
    }

    #[test]
    fn page_query_is_a_closed_allow_list() {
        assert_eq!(page_query(None, "after").ok().as_deref(), Some(""));
        assert_eq!(
            page_query(Some("after=c1&limit=10"), "after").ok().as_deref(),
            Some("after=c1&limit=10")
        );
        assert_eq!(page_query(Some("before=c2"), "before").ok().as_deref(), Some("before=c2"));
        for refused in [
            "api_key=secret",
            "x-api-key=secret",
            "before=c1",
            "limit=0",
            "limit=101",
            "limit=ten",
            "after=a&after=b",
            "after=",
        ] {
            let refusal = page_query(Some(refused), "after").expect_err(refused);
            assert_eq!(refusal.status(), StatusCode::BAD_REQUEST, "{refused}");
        }
    }

    #[test]
    fn identifiers_cannot_add_a_path_or_query() {
        assert!(is_identifier("run-1"));
        assert!(is_identifier("0b6f4a52-5d0e-4c2a-9d7b-2a1f3c4d5e6f"));
        assert!(is_identifier("thr_abc"));
        let too_long = "x".repeat(129);
        for refused in ["", "..", "a/b", "a?b", "a%2Fb", "a b", "a#b", too_long.as_str()] {
            assert!(!is_identifier(refused), "{refused}");
        }
    }
}
