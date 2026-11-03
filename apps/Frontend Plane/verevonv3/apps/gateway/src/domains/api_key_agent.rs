//! `x-api-key` ingress for browser agent runs (INNH-10 GW-1).
//!
//! A narrow public surface for API-key callers (no session, no cookie):
//!
//! ```text
//! POST   /api/v1/api-key/agent/runs                 start a run
//! POST   /api/v1/api-key/agent/runs/{run_id}/step   one step
//! DELETE /api/v1/api-key/agent/runs/{run_id}        close the run
//! ```
//!
//! The gateway composes the chain of `.plan-work/quarry-w1w2/INNH-10-design.md`
//! section 2 (OD-3: the caller never sees a grant):
//!
//! 1. exchange the key at auth-core (`POST /api/{audience}/api-key-exchange`, CP-1) with the
//!    gateway's own service credentials, once per audience (`browser-broker`, `quarry`). The
//!    exchanged token is cached only until its `exp` (at most 120 s, less a skew), keyed by a
//!    SHA-256 of the key plus org and audience. The key itself is never stored, never logged
//!    and never sent anywhere but auth-core;
//! 2. acquire a key grant at model-gateway `POST /v1/browser/key-grants` (Model Plane ask
//!    MP-3, built against the documented contract) with the broker-audience key token;
//! 3. start the Quarry run (`POST /v1/agent/runs`) with the quarry-audience key token and the
//!    grant id, then step and close it;
//! 4. revoke the grant (`DELETE /v1/browser/key-grants/{grant_id}`) when the run closes, when
//!    the start fails after the grant was issued, and when Quarry reports the run gone.
//!
//! Every upstream request is built fresh here: no caller header is copied, so a caller's
//! `Authorization`, `x-browser-authorization`, `cookie` or scoping header (`x-org-id`,
//! `x-user-id`, ...) never reaches an upstream. The org comes from the request body on
//! start (auth-core refuses a key bound to another org) and from the gateway's own run
//! binding afterwards; a step or close by a different key answers 404 like an unknown run.
//!
//! Per-key ingress rate limit (OD-11: 600 per hour by default) on every route, keyed by
//! the key's hash, before any upstream call.
//!
//! Dark by default: `API_KEY_AGENT_RUNS` (exactly `true` or `1`) mounts the routes; off,
//! [`router`] returns an empty router and every path is the gateway's ordinary 404.
//! Configuration is read once when the router is built, never in a handler.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header::RETRY_AFTER, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, post},
    Json, Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::AppState,
    envelope::{error_with_details, ok, unwrap_data},
    rate_limit::RateLimiter,
};

/// The caller's credential header. Read here, sent only to auth-core.
const API_KEY_HEADER: &str = "x-api-key";
/// Better Auth keys are far shorter; auth-core refuses more than 512 too.
const MAX_API_KEY_LEN: usize = 512;
/// The only scope an exchanged key token may carry (design section 3).
const BROWSER_AGENT_SCOPE: &str = "browser:agent";
/// Default ingress rate per key (OD-11).
const DEFAULT_RATE_PER_HOUR: u32 = 600;
/// Upper bound of a host list (OD-6, the key's host ceiling).
const MAX_HOSTS: usize = 32;
/// Absolute per-run budget limits (CP-2: 10 USD, 500 steps, 900 s).
const MAX_BUDGET_MICRO_USD: u64 = 10_000_000;
const MAX_BUDGET_STEPS: u32 = 500;
const MAX_BUDGET_RUNTIME_S: u32 = 900;
/// An exchanged token lives at most 120 s (OD-9); drop it this much before its `exp`.
const TOKEN_MAX_TTL: Duration = Duration::from_secs(120);
const TOKEN_EXPIRY_SKEW: Duration = Duration::from_secs(15);
/// Hard bound on the token cache; expired entries are swept before this is hit.
const MAX_CACHED_TOKENS: usize = 4096;
/// A key grant lives at most 900 s and never renews (OD-9); a run binding is kept a
/// little longer so a late close still revokes.
const RUN_BINDING_TTL: Duration = Duration::from_secs(900 + 120);
/// Hard bound on live run bindings in this gateway process.
const MAX_RUN_BINDINGS: usize = 4096;
/// A browser step can outlast the shared client's 25 s ceiling.
const STEP_TIMEOUT: Duration = Duration::from_secs(120);

/// `API_KEY_AGENT_RUNS` and the service credentials the gateway presents at auth-core.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiKeyAgentConfig {
    /// `API_KEY_AGENT_RUNS`: mounts the routes. Default off.
    pub(crate) enabled: bool,
    /// `API_KEY_AGENT_SERVICE_ID` (default `verevon-gateway`): the registry principal whose
    /// `apiKeyExchange` lists `quarry` and `browser-broker`.
    pub(crate) service_id: String,
    /// `API_KEY_AGENT_SERVICE_API_KEY`: that principal's credential. Empty with the flag on
    /// answers 503 `api_key_agent_not_configured` and calls nothing.
    pub(crate) service_api_key: String,
    /// `API_KEY_AGENT_RATE_PER_HOUR` (default 600): ingress requests per key per hour.
    pub(crate) rate_per_hour: u32,
}

impl ApiKeyAgentConfig {
    pub(crate) fn from_env() -> Self {
        let read = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_owned());
        let enabled = read("API_KEY_AGENT_RUNS").is_some_and(|v| v == "true" || v == "1");
        let service_id = read("API_KEY_AGENT_SERVICE_ID")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "verevon-gateway".to_owned());
        let service_api_key = read("API_KEY_AGENT_SERVICE_API_KEY").unwrap_or_default();
        let rate_per_hour = read("API_KEY_AGENT_RATE_PER_HOUR")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_RATE_PER_HOUR);
        Self {
            enabled,
            service_id,
            service_api_key,
            rate_per_hour,
        }
    }

    fn configured(&self) -> bool {
        !self.service_id.trim().is_empty() && !self.service_api_key.trim().is_empty()
    }
}

/// The two audiences auth-core exchanges a key for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyAudience {
    BrowserBroker,
    Quarry,
}

impl KeyAudience {
    const fn slug(self) -> &'static str {
        match self {
            Self::BrowserBroker => "browser-broker",
            Self::Quarry => "quarry",
        }
    }
}

struct CachedKeyToken {
    token: String,
    expires_at: Instant,
}

/// What the gateway remembers about a run it started for a key. No key, no token.
#[derive(Clone, Debug)]
struct RunBinding {
    key_hash: String,
    org_id: String,
    grant_id: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct KeyAgentState {
    app: AppState,
    config: Arc<ApiKeyAgentConfig>,
    tokens: Arc<DashMap<String, CachedKeyToken>>,
    runs: Arc<DashMap<String, RunBinding>>,
    limiter: RateLimiter,
}

/// Mounted only when `API_KEY_AGENT_RUNS` is on. `main.rs`'s `build_router` merges it
/// (one `.merge(domains::api_key_agent::router(state.clone()))` line, integration step).
#[allow(
    dead_code,
    reason = "merged into build_router by the integration step (main.rs is outside W2-FE-GW1)"
)]
pub(crate) fn router(state: AppState) -> Router<AppState> {
    router_with_config(state, ApiKeyAgentConfig::from_env())
}

fn router_with_config(state: AppState, config: ApiKeyAgentConfig) -> Router<AppState> {
    if !config.enabled {
        return Router::new();
    }
    let rpm = f64::from(config.rate_per_hour) / 60.0;
    let limiter = RateLimiter::from_cache_with_rpm(&state.cache, rpm);
    let key_state = KeyAgentState {
        app: state,
        config: Arc::new(config),
        tokens: Arc::new(DashMap::new()),
        runs: Arc::new(DashMap::new()),
        limiter,
    };
    Router::new()
        .route("/api/v1/api-key/agent/runs", post(start_run))
        .route("/api/v1/api-key/agent/runs/{run_id}/step", post(step_run))
        .route("/api/v1/api-key/agent/runs/{run_id}", delete(close_run))
        .with_state(key_state)
}

// ---------------------------------------------------------------------------
// Request shapes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct KeyRunBudget {
    micro_usd: u64,
    max_steps: u32,
    max_runtime_s: u32,
}

impl KeyRunBudget {
    fn valid(&self) -> bool {
        (1..=MAX_BUDGET_MICRO_USD).contains(&self.micro_usd)
            && (1..=MAX_BUDGET_STEPS).contains(&self.max_steps)
            && (1..=MAX_BUDGET_RUNTIME_S).contains(&self.max_runtime_s)
    }

    /// Field-wise minimum: the broker may grant less than was asked, never more.
    fn tightest(self, other: Self) -> Self {
        Self {
            micro_usd: self.micro_usd.min(other.micro_usd),
            max_steps: self.max_steps.min(other.max_steps),
            max_runtime_s: self.max_runtime_s.min(other.max_runtime_s),
        }
    }
}

/// `POST /api/v1/api-key/agent/runs` (INNH-10 design section 12, plus `org_id`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRunRequest {
    org_id: String,
    allowed_domains: Vec<String>,
    budget: KeyRunBudget,
    #[serde(default)]
    zdr: bool,
    #[serde(default)]
    start_url: Option<String>,
    #[serde(default)]
    initiated_by: Option<String>,
}

fn valid_org_id(org_id: &str) -> bool {
    !org_id.is_empty()
        && org_id.len() <= 128
        && org_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// A host the broker can canonicalise: no scheme, path, port, wildcard or credentials.
/// The broker stays the host authority; this only refuses obvious garbage early.
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        && !host.starts_with('.')
        && !host.starts_with('-')
}

fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 128
        && run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn validate_start(request: &StartRunRequest) -> Result<(), &'static str> {
    if !valid_org_id(request.org_id.trim()) {
        return Err("org_id_invalid");
    }
    if request.allowed_domains.is_empty() || request.allowed_domains.len() > MAX_HOSTS {
        return Err("allowed_domains_invalid");
    }
    if !request.allowed_domains.iter().all(|host| valid_host(host)) {
        return Err("allowed_domains_invalid");
    }
    if !request.budget.valid() {
        return Err("budget_invalid");
    }
    if request
        .initiated_by
        .as_deref()
        .is_some_and(|value| value != "agent")
    {
        return Err("initiated_by_invalid");
    }
    if let Some(start_url) = request.start_url.as_deref() {
        let parsed = url::Url::parse(start_url).map_err(|_| "start_url_invalid")?;
        let http = matches!(parsed.scheme(), "http" | "https");
        if !http || parsed.host_str().is_none() || !parsed.username().is_empty() {
            return Err("start_url_invalid");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Key handling
// ---------------------------------------------------------------------------

/// SHA-256 of the key, hex. The only form of the key that is kept or used as a map key.
fn key_hash(api_key: &str) -> String {
    hex::encode(Sha256::digest(api_key.as_bytes()))
}

/// A short, non-reversible reference for logs (a prefix of the hash).
fn key_ref(key_hash: &str) -> &str {
    &key_hash[..key_hash.len().min(12)]
}

/// The caller's key, or a refusal. Printable ASCII, no surrounding whitespace.
fn read_api_key(headers: &HeaderMap) -> Result<String, Response> {
    let Some(raw) = headers.get(API_KEY_HEADER) else {
        return Err(refuse(
            StatusCode::UNAUTHORIZED,
            "api_key_missing",
            "An x-api-key header is required.",
            "api_key_missing",
        ));
    };
    let value = raw.to_str().unwrap_or_default();
    let printable = value.bytes().all(|b| b.is_ascii_graphic());
    if value.is_empty() || value.len() > MAX_API_KEY_LEN || !printable {
        return Err(refuse(
            StatusCode::UNAUTHORIZED,
            "api_key_invalid",
            "The x-api-key header is not a valid API key.",
            "api_key_invalid",
        ));
    }
    Ok(value.to_owned())
}

fn token_cache_key(key_hash: &str, org_id: &str, audience: KeyAudience) -> String {
    format!("{key_hash}\u{0}{org_id}\u{0}{}", audience.slug())
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

fn refuse(status: StatusCode, code: &'static str, message: &str, reason: &str) -> Response {
    (
        status,
        Json(error_with_details(
            code,
            message,
            json!({ "reason": reason }),
        )),
    )
        .into_response()
}

fn not_found() -> Response {
    refuse(
        StatusCode::NOT_FOUND,
        "not_found",
        "No such run for this API key.",
        "run_not_found",
    )
}

fn unavailable(code: &'static str, reason: &str) -> Response {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        code,
        "The service behind this route is unavailable.",
        reason,
    )
}

/// A reason an upstream gave, when it is a plain snake_case token (never free text).
fn upstream_reason(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/details/reason"),
        body.pointer("/details/reason"),
        body.get("reason"),
        body.pointer("/error/code"),
        body.get("code"),
        body.pointer("/message/code"),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|reason| safe_reason(reason))
        .map(str::to_owned)
}

fn safe_reason(reason: &str) -> bool {
    !reason.is_empty()
        && reason.len() <= 64
        && reason
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// An upstream refusal, rebuilt: the status class is kept for 4xx, the code is ours, the
/// reason is the upstream's when it is a plain token. Upstream messages never pass through.
fn upstream_refusal(
    status: StatusCode,
    body: &Value,
    code: &'static str,
    fallback_reason: &str,
) -> Response {
    let reason = upstream_reason(body).unwrap_or_else(|| fallback_reason.to_owned());
    if status.is_client_error() {
        refuse(status, code, "The request was refused upstream.", &reason)
    } else {
        refuse(
            StatusCode::BAD_GATEWAY,
            code,
            "The upstream service failed.",
            &reason,
        )
    }
}

fn rate_limited(retry_after: u64) -> Response {
    let mut response = refuse(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "Too many requests for this API key.",
        "api_key_rate_limited",
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after.max(1).to_string()) {
        response.headers_mut().insert(RETRY_AFTER, value);
    }
    response
}

// ---------------------------------------------------------------------------
// Upstream calls (each request built fresh; no caller header is ever copied)
// ---------------------------------------------------------------------------

async fn send_json(builder: reqwest::RequestBuilder) -> Option<(StatusCode, Value)> {
    let response = builder.send().await.ok()?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = response.bytes().await.ok()?;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    Some((status, value))
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

impl KeyAgentState {
    fn base(url: &str) -> &str {
        url.trim_end_matches('/')
    }

    /// The common gate of every route: configured, key present, within the key's rate.
    async fn admit(&self, headers: &HeaderMap) -> Result<(String, String), Response> {
        if !self.config.configured() {
            return Err(unavailable(
                "api_key_agent_not_configured",
                "api_key_agent_not_configured",
            ));
        }
        let api_key = read_api_key(headers)?;
        let hash = key_hash(&api_key);
        if let Err(retry_after) = self
            .limiter
            .try_acquire(&format!("api_key_agent:{hash}"))
            .await
        {
            return Err(rate_limited(retry_after));
        }
        Ok((api_key, hash))
    }

    /// An exchanged key token for `audience`, from the cache while it is fresh.
    async fn exchange(
        &self,
        api_key: &str,
        hash: &str,
        org_id: &str,
        audience: KeyAudience,
    ) -> Result<String, Response> {
        let cache_key = token_cache_key(hash, org_id, audience);
        let now = Instant::now();
        let cached = self
            .tokens
            .get(&cache_key)
            .filter(|cached| cached.expires_at > now)
            .map(|cached| cached.token.clone());
        if let Some(token) = cached {
            return Ok(token);
        }
        self.tokens.remove(&cache_key);

        let url = format!(
            "{}/api/{}/api-key-exchange",
            Self::base(&self.app.auth_core_url),
            audience.slug()
        );
        let request = self
            .app
            .client
            .post(url)
            .header("x-service-id", self.config.service_id.as_str())
            .header("x-service-api-key", self.config.service_api_key.as_str())
            .json(&json!({
                "apiKey": api_key,
                "orgId": org_id,
                "scopes": [BROWSER_AGENT_SCOPE],
            }));
        let Some((status, body)) = send_json(request).await else {
            tracing::warn!(
                key_ref = %key_ref(hash),
                audience = audience.slug(),
                "api-key exchange transport failure"
            );
            return Err(unavailable(
                "api_key_exchange_unavailable",
                "exchange_unavailable",
            ));
        };
        if !status.is_success() {
            tracing::info!(
                key_ref = %key_ref(hash),
                audience = audience.slug(),
                status = status.as_u16(),
                "api-key exchange refused"
            );
            return Err(match status {
                // auth-core answers 404 while its exchange flag is off: the path is dark.
                StatusCode::NOT_FOUND => {
                    unavailable("api_key_exchange_unavailable", "exchange_disabled")
                }
                StatusCode::BAD_REQUEST => refuse(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The API key exchange refused the request.",
                    &upstream_reason(&body).unwrap_or_else(|| "invalid_request".to_owned()),
                ),
                StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::TOO_MANY_REQUESTS => {
                    upstream_refusal(status, &body, "api_key_refused", "api_key_refused")
                }
                _ => unavailable("api_key_exchange_unavailable", "exchange_unavailable"),
            });
        }

        let data = unwrap_data(&body);
        let token = data
            .get("token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_owned);
        let Some(token) = token else {
            return Err(unavailable(
                "api_key_exchange_unavailable",
                "exchange_response_invalid",
            ));
        };
        let ttl = data
            .get("expiresInSeconds")
            .or_else(|| data.get("expires_in"))
            .and_then(Value::as_u64)
            .map(Duration::from_secs)
            .unwrap_or(Duration::ZERO)
            .min(TOKEN_MAX_TTL);
        if ttl > TOKEN_EXPIRY_SKEW {
            if self.tokens.len() >= MAX_CACHED_TOKENS {
                self.tokens.retain(|_, cached| cached.expires_at > now);
            }
            if self.tokens.len() < MAX_CACHED_TOKENS {
                self.tokens.insert(
                    cache_key,
                    CachedKeyToken {
                        token: token.clone(),
                        expires_at: now + (ttl - TOKEN_EXPIRY_SKEW),
                    },
                );
            }
        }
        Ok(token)
    }

    /// Best effort: a failed revoke still leaves the grant's own TTL (at most 900 s).
    async fn revoke_grant(&self, api_key: &str, hash: &str, org_id: &str, grant_id: &str) {
        let token = match self
            .exchange(api_key, hash, org_id, KeyAudience::BrowserBroker)
            .await
        {
            Ok(token) => token,
            Err(_) => {
                tracing::warn!(key_ref = %key_ref(hash), "key grant revoke skipped: no broker token");
                return;
            }
        };
        let url = format!(
            "{}/v1/browser/key-grants/{}",
            Self::base(&self.app.model_gateway_url),
            urlencoding::encode(grant_id)
        );
        let request = self
            .app
            .client
            .delete(url)
            .header("x-browser-authorization", bearer(&token));
        match send_json(request).await {
            Some((status, _)) if status.is_success() || status == StatusCode::NOT_FOUND => {}
            Some((status, _)) => {
                tracing::warn!(
                    key_ref = %key_ref(hash),
                    status = status.as_u16(),
                    "key grant revoke refused"
                );
            }
            None => {
                tracing::warn!(key_ref = %key_ref(hash), "key grant revoke transport failure");
            }
        }
    }

    async fn quarry(
        &self,
        method: reqwest::Method,
        path: &str,
        token: &str,
        body: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Option<(StatusCode, Value)> {
        let url = format!("{}{}", Self::base(&self.app.quarry_edge_url), path);
        let mut request = self
            .app
            .client
            .request(method, url)
            .header("authorization", bearer(token));
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        send_json(request).await
    }

    /// The caller's live binding for `run_id`, or `None` (unknown, expired or another key's).
    fn binding(&self, run_id: &str, hash: &str) -> Option<RunBinding> {
        let binding = self.runs.get(run_id).map(|entry| entry.value().clone())?;
        if binding.expires_at <= Instant::now() {
            self.runs.remove(run_id);
            return None;
        }
        (binding.key_hash == hash).then_some(binding)
    }

    fn has_room_for_run(&self) -> bool {
        if self.runs.len() >= MAX_RUN_BINDINGS {
            let now = Instant::now();
            self.runs.retain(|_, binding| binding.expires_at > now);
        }
        self.runs.len() < MAX_RUN_BINDINGS
    }
}

/// What model-gateway's key-grant door answers (MP-3): `{ grant_id, expires_at,
/// allowed_domains? (omitted under ZDR), budget, key_id }`, optionally in `{ data }`.
struct IssuedKeyGrant {
    grant_id: String,
    expires_at: Option<String>,
    allowed_domains: Option<Vec<String>>,
    budget: Option<KeyRunBudget>,
    zdr: bool,
}

fn parse_key_grant(body: &Value) -> Option<IssuedKeyGrant> {
    let data = unwrap_data(body);
    let grant_id = data
        .get("grant_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)?
        .to_owned();
    let allowed_domains = data
        .get("allowed_domains")
        .and_then(|hosts| serde_json::from_value::<Vec<String>>(hosts.clone()).ok());
    let budget = data
        .get("budget")
        .and_then(|budget| serde_json::from_value::<KeyRunBudget>(budget.clone()).ok());
    // The broker marks a zero-retention grant with a `z_` id (design section 5).
    let zdr = grant_id.starts_with("z_") || data.get("zdr").and_then(Value::as_bool) == Some(true);
    let expires_at = data
        .get("expires_at")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(IssuedKeyGrant {
        grant_id,
        expires_at,
        allowed_domains,
        budget,
        zdr,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/api-key/agent/runs`
async fn start_run(State(ks): State<KeyAgentState>, headers: HeaderMap, body: Bytes) -> Response {
    let (api_key, hash) = match ks.admit(&headers).await {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    let request: StartRunRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The body must be { org_id, allowed_domains, budget, zdr?, start_url?, initiated_by? }.",
                "body_invalid",
            )
        }
    };
    if let Err(reason) = validate_start(&request) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The run request is invalid.",
            reason,
        );
    }
    if !ks.has_room_for_run() {
        return unavailable("api_key_agent_busy", "run_table_full");
    }
    let org_id = request.org_id.trim().to_owned();

    // 1. The broker-audience key token, then the key grant (MP-3).
    let broker_token = match ks
        .exchange(&api_key, &hash, &org_id, KeyAudience::BrowserBroker)
        .await
    {
        Ok(token) => token,
        Err(response) => return response,
    };
    let grant_url = format!(
        "{}/v1/browser/key-grants",
        KeyAgentState::base(&ks.app.model_gateway_url)
    );
    let grant_request = ks
        .app
        .client
        .post(grant_url)
        .header("x-browser-authorization", bearer(&broker_token))
        .json(&json!({
            "allowed_domains": request.allowed_domains,
            "budget": request.budget,
            "zdr": request.zdr,
        }));
    let grant = match send_json(grant_request).await {
        Some((status, body)) if status.is_success() => match parse_key_grant(&body) {
            Some(grant) => grant,
            None => {
                return refuse(
                    StatusCode::BAD_GATEWAY,
                    "api_key_grant_invalid",
                    "The browser grant answer was unreadable.",
                    "grant_response_invalid",
                )
            }
        },
        Some((status, body)) => {
            tracing::info!(
                key_ref = %key_ref(&hash),
                status = status.as_u16(),
                "api-key grant refused"
            );
            return upstream_refusal(status, &body, "api_key_grant_refused", "grant_refused");
        }
        None => return unavailable("api_key_grant_unavailable", "grant_unavailable"),
    };

    // From here on every failure revokes the grant.
    let zdr = request.zdr || grant.zdr;
    let budget = grant
        .budget
        .map_or(request.budget, |granted| granted.tightest(request.budget));
    let hosts = grant
        .allowed_domains
        .clone()
        .unwrap_or_else(|| request.allowed_domains.clone());

    // 2. The quarry-audience key token, then the run.
    let quarry_token = match ks
        .exchange(&api_key, &hash, &org_id, KeyAudience::Quarry)
        .await
    {
        Ok(token) => token,
        Err(response) => {
            ks.revoke_grant(&api_key, &hash, &org_id, &grant.grant_id)
                .await;
            return response;
        }
    };
    let start_body = json!({
        "constraints": {
            "max_steps": budget.max_steps,
            "allowed_domains": hosts,
            "max_runtime_s": budget.max_runtime_s,
            "max_cost_usd": budget.micro_usd as f64 / 1_000_000.0,
        },
        "zdr": zdr,
        "grant_id": grant.grant_id,
        "initiated_by": "agent",
    });
    let started = ks
        .quarry(
            reqwest::Method::POST,
            "/v1/agent/runs",
            &quarry_token,
            Some(&start_body),
            None,
        )
        .await;
    let run_id = match started {
        Some((status, body)) if status.is_success() => {
            let run_id = unwrap_data(&body)
                .get("run_id")
                .and_then(Value::as_str)
                .filter(|id| valid_run_id(id))
                .map(str::to_owned);
            match run_id {
                Some(run_id) => run_id,
                None => {
                    ks.revoke_grant(&api_key, &hash, &org_id, &grant.grant_id)
                        .await;
                    return refuse(
                        StatusCode::BAD_GATEWAY,
                        "agent_run_invalid",
                        "Quarry did not return a run id.",
                        "run_response_invalid",
                    );
                }
            }
        }
        Some((status, body)) => {
            tracing::info!(
                key_ref = %key_ref(&hash),
                status = status.as_u16(),
                "api-key agent run start refused"
            );
            ks.revoke_grant(&api_key, &hash, &org_id, &grant.grant_id)
                .await;
            return upstream_refusal(status, &body, "agent_run_refused", "run_refused");
        }
        None => {
            ks.revoke_grant(&api_key, &hash, &org_id, &grant.grant_id)
                .await;
            return unavailable("agent_run_unavailable", "quarry_unavailable");
        }
    };
    ks.runs.insert(
        run_id.clone(),
        RunBinding {
            key_hash: hash.clone(),
            org_id: org_id.clone(),
            grant_id: grant.grant_id.clone(),
            expires_at: Instant::now() + RUN_BINDING_TTL,
        },
    );
    tracing::info!(key_ref = %key_ref(&hash), zdr, "api-key agent run started");

    // 3. Optional first navigation. A failure closes the run and revokes the grant.
    let mut initial_step = None;
    if let Some(start_url) = request.start_url.as_deref() {
        let step_body = json!({
            "action": { "type": "navigate", "url": start_url },
            "instruction": "Open the start page of an API-key agent run.",
        });
        let step_path = format!("/v1/agent/runs/{}/step", urlencoding::encode(&run_id));
        let stepped = ks
            .quarry(
                reqwest::Method::POST,
                &step_path,
                &quarry_token,
                Some(&step_body),
                Some(STEP_TIMEOUT),
            )
            .await;
        match stepped {
            Some((status, body)) if status.is_success() => {
                initial_step = Some(unwrap_data(&body));
            }
            failure => {
                let close_path = format!("/v1/agent/runs/{}", urlencoding::encode(&run_id));
                let _ = ks
                    .quarry(
                        reqwest::Method::DELETE,
                        &close_path,
                        &quarry_token,
                        None,
                        None,
                    )
                    .await;
                ks.runs.remove(&run_id);
                ks.revoke_grant(&api_key, &hash, &org_id, &grant.grant_id)
                    .await;
                return match failure {
                    Some((status, body)) => {
                        upstream_refusal(status, &body, "agent_run_refused", "start_step_refused")
                    }
                    None => unavailable("agent_run_unavailable", "quarry_unavailable"),
                };
            }
        }
    }

    let mut data = json!({
        "run_id": run_id,
        "budget": budget,
        "zdr": zdr,
    });
    if let Some(expires_at) = grant.expires_at {
        data["expires_at"] = Value::String(expires_at);
    }
    // Hosts are omitted under ZDR, as the broker's own door does.
    if !zdr {
        data["allowed_domains"] = json!(hosts);
    }
    if let Some(step) = initial_step {
        data["initial_step"] = step;
    }
    (StatusCode::CREATED, Json(ok(data))).into_response()
}

/// `POST /api/v1/api-key/agent/runs/{run_id}/step`: the body (a JSON object) goes to
/// Quarry as is; Quarry validates it and enforces the grant on every step.
async fn step_run(
    State(ks): State<KeyAgentState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    body: Bytes,
) -> Response {
    let (api_key, hash) = match ks.admit(&headers).await {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    let Some(binding) = ks.binding(&run_id, &hash) else {
        return not_found();
    };
    let step_body: Value = match serde_json::from_slice(&body) {
        Ok(value @ Value::Object(_)) => value,
        _ => {
            return refuse(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The step body must be a JSON object.",
                "body_invalid",
            )
        }
    };
    let token = match ks
        .exchange(&api_key, &hash, &binding.org_id, KeyAudience::Quarry)
        .await
    {
        Ok(token) => token,
        Err(response) => return response,
    };
    let path = format!("/v1/agent/runs/{}/step", urlencoding::encode(&run_id));
    let stepped = ks
        .quarry(
            reqwest::Method::POST,
            &path,
            &token,
            Some(&step_body),
            Some(STEP_TIMEOUT),
        )
        .await;
    match stepped {
        Some((status, body)) if status.is_success() => {
            (StatusCode::OK, Json(ok(unwrap_data(&body)))).into_response()
        }
        Some((status, body)) => {
            // The run is gone upstream (closed, swept or expired): the grant has nothing
            // left to back, so it is revoked now rather than at its TTL.
            if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
                ks.runs.remove(&run_id);
                ks.revoke_grant(&api_key, &hash, &binding.org_id, &binding.grant_id)
                    .await;
            }
            upstream_refusal(status, &body, "agent_step_refused", "step_refused")
        }
        None => unavailable("agent_run_unavailable", "quarry_unavailable"),
    }
}

/// `DELETE /api/v1/api-key/agent/runs/{run_id}`: closes the Quarry run and always revokes
/// the grant, whatever Quarry answered.
async fn close_run(
    State(ks): State<KeyAgentState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    let (api_key, hash) = match ks.admit(&headers).await {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    let Some(binding) = ks.binding(&run_id, &hash) else {
        return not_found();
    };
    ks.runs.remove(&run_id);
    let path = format!("/v1/agent/runs/{}", urlencoding::encode(&run_id));
    let closed = match ks
        .exchange(&api_key, &hash, &binding.org_id, KeyAudience::Quarry)
        .await
    {
        Ok(token) => {
            ks.quarry(reqwest::Method::DELETE, &path, &token, None, None)
                .await
        }
        Err(_) => None,
    };
    ks.revoke_grant(&api_key, &hash, &binding.org_id, &binding.grant_id)
        .await;
    match closed {
        Some((status, _)) if status.is_success() || status == StatusCode::NOT_FOUND => (
            StatusCode::OK,
            Json(ok(json!({ "run_id": run_id, "closed": true }))),
        )
            .into_response(),
        Some((status, body)) => {
            upstream_refusal(status, &body, "agent_run_refused", "close_refused")
        }
        None => unavailable("agent_run_unavailable", "quarry_unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{header as wm_header, method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    const KEY: &str = "test-key-one";
    const OTHER_KEY: &str = "test-key-two";
    const SERVICE_KEY: &str = "svc-cred-test";
    const RUNS: &str = "/api/v1/api-key/agent/runs";

    fn config(rate_per_hour: u32) -> ApiKeyAgentConfig {
        ApiKeyAgentConfig {
            enabled: true,
            service_id: "verevon-gateway".to_owned(),
            service_api_key: SERVICE_KEY.to_owned(),
            rate_per_hour,
        }
    }

    struct Harness {
        auth: MockServer,
        model_gateway: MockServer,
        quarry: MockServer,
    }

    impl Harness {
        async fn start() -> Self {
            Self {
                auth: MockServer::start().await,
                model_gateway: MockServer::start().await,
                quarry: MockServer::start().await,
            }
        }

        fn state(&self) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.model_gateway_url = self.model_gateway.uri();
            state.quarry_edge_url = self.quarry.uri();
            state
        }

        fn app(&self, config: ApiKeyAgentConfig) -> Router {
            let state = self.state();
            router_with_config(state.clone(), config).with_state(state)
        }

        async fn mount_exchange(&self, audience: &str, token: &str, expect: u64) {
            Mock::given(wm_method("POST"))
                .and(wm_path(format!("/api/{audience}/api-key-exchange")))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-service-api-key", SERVICE_KEY))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "token": token,
                    "expiresAt": "2026-11-03T12:02:00Z",
                    "expiresInSeconds": 120,
                    "issuer": "auth-core",
                    "audience": audience,
                })))
                .expect(expect)
                .mount(&self.auth)
                .await;
        }

        async fn mount_grant(&self, grant_id: &str, with_hosts: bool) {
            let mut data = json!({
                "grant_id": grant_id,
                "expires_at": "2026-11-03T12:10:00Z",
                "budget": {"micro_usd": 500_000, "max_steps": 20, "max_runtime_s": 300},
                "key_id": "key-1",
            });
            if with_hosts {
                data["allowed_domains"] = json!(["example.com"]);
            }
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/key-grants"))
                .and(wm_header("x-browser-authorization", "Bearer broker-token"))
                .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "data": data })))
                .mount(&self.model_gateway)
                .await;
        }

        async fn mount_revoke(&self, grant_id: &str, expect: u64) {
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/browser/key-grants/{grant_id}")))
                .and(wm_header("x-browser-authorization", "Bearer broker-token"))
                .respond_with(ResponseTemplate::new(204))
                .expect(expect)
                .mount(&self.model_gateway)
                .await;
        }

        async fn mount_quarry_start(&self, status: u16, body: Value) {
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs"))
                .and(wm_header("authorization", "Bearer quarry-token"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&self.quarry)
                .await;
        }

        async fn mount_quarry_step(&self, run_id: &str, status: u16, body: Value) {
            Mock::given(wm_method("POST"))
                .and(wm_path(format!("/v1/agent/runs/{run_id}/step")))
                .and(wm_header("authorization", "Bearer quarry-token"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&self.quarry)
                .await;
        }

        async fn mount_quarry_close(&self, run_id: &str) {
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/agent/runs/{run_id}")))
                .and(wm_header("authorization", "Bearer quarry-token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"data": {"closed": true}})),
                )
                .mount(&self.quarry)
                .await;
        }

        /// The happy-path doubles for a run `run-1` under grant `g-1`.
        async fn mount_happy(&self) {
            self.mount_exchange("browser-broker", "broker-token", 1)
                .await;
            self.mount_exchange("quarry", "quarry-token", 1).await;
            self.mount_grant("g-1", true).await;
            self.mount_quarry_start(201, json!({"data": {"run_id": "run-1"}}))
                .await;
        }

        async fn all_upstream_requests(&self) -> Vec<(String, wiremock::Request)> {
            let mut all = Vec::new();
            for (name, server) in [
                ("auth", &self.auth),
                ("model_gateway", &self.model_gateway),
                ("quarry", &self.quarry),
            ] {
                for request in server.received_requests().await.unwrap_or_default() {
                    all.push((name.to_owned(), request));
                }
            }
            all
        }
    }

    fn start_body() -> Value {
        json!({
            "org_id": "org-1",
            "allowed_domains": ["example.com"],
            "budget": {"micro_usd": 1_000_000, "max_steps": 50, "max_runtime_s": 600},
            "initiated_by": "agent",
        })
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        key: Option<&str>,
        extra: &[(&str, &str)],
        body: Option<Value>,
    ) -> (u16, HeaderMap, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(key) = key {
            builder = builder.header(API_KEY_HEADER, key);
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
        let response = app
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, value)
    }

    #[tokio::test]
    async fn flag_off_mounts_no_route() {
        let harness = Harness::start().await;
        let app = harness.app(ApiKeyAgentConfig::default());
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 404);
        assert!(harness.all_upstream_requests().await.is_empty());
    }

    #[test]
    fn the_default_config_is_off_and_unconfigured() {
        let default = ApiKeyAgentConfig::default();
        assert!(!default.enabled);
        assert!(!default.configured());
    }

    #[tokio::test]
    async fn flag_on_without_service_credentials_is_503_and_calls_nothing() {
        let harness = Harness::start().await;
        let mut unconfigured = config(600);
        unconfigured.service_api_key = String::new();
        let app = harness.app(unconfigured);
        let (status, _, body) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["code"], "api_key_agent_not_configured");
        assert!(harness.all_upstream_requests().await.is_empty());
    }

    #[tokio::test]
    async fn a_missing_key_is_401_and_calls_nothing() {
        let harness = Harness::start().await;
        let app = harness.app(config(600));
        let (status, _, body) = call(
            &app,
            "POST",
            RUNS,
            None,
            &[("authorization", "Bearer someone-elses-session")],
            Some(start_body()),
        )
        .await;
        assert_eq!(status, 401);
        assert_eq!(body["error"]["code"], "api_key_missing");
        assert!(harness.all_upstream_requests().await.is_empty());
    }

    #[tokio::test]
    async fn an_invalid_body_is_400_and_calls_nothing() {
        let harness = Harness::start().await;
        let app = harness.app(config(600));
        let budget = json!({"micro_usd": 1, "max_steps": 1, "max_runtime_s": 1});
        for bad in [
            json!({"org_id": "org-1", "allowed_domains": [], "budget": budget}),
            json!({"org_id": "org-1", "allowed_domains": ["*.example.com"], "budget": budget}),
            json!({"org_id": "org-1", "allowed_domains": ["example.com"], "budget": {"micro_usd": 0, "max_steps": 1, "max_runtime_s": 1}}),
            json!({"org_id": "org-1", "allowed_domains": ["example.com"], "budget": {"micro_usd": 1, "max_steps": 1, "max_runtime_s": 901}}),
            json!({"org_id": "org-1", "allowed_domains": ["example.com"], "budget": budget, "initiated_by": "human"}),
            json!({"org_id": "org-1", "allowed_domains": ["example.com"], "budget": budget, "grant_id": "smuggled"}),
            json!({"org_id": "org-1", "allowed_domains": ["example.com"], "budget": budget, "start_url": "file:///etc/passwd"}),
            json!({"org_id": "", "allowed_domains": ["example.com"], "budget": budget}),
        ] {
            let (status, _, body) =
                call(&app, "POST", RUNS, Some(KEY), &[], Some(bad.clone())).await;
            assert_eq!(status, 400, "{bad} -> {body}");
            assert_eq!(body["error"]["code"], "invalid_request");
        }
        assert!(harness.all_upstream_requests().await.is_empty());
    }

    #[tokio::test]
    async fn start_exchanges_acquires_and_starts_with_caller_headers_stripped() {
        let harness = Harness::start().await;
        harness.mount_happy().await;
        let app = harness.app(config(600));
        let (status, _, body) = call(
            &app,
            "POST",
            RUNS,
            Some(KEY),
            &[
                ("authorization", "Bearer CALLER-AUTHORIZATION"),
                (
                    "x-browser-authorization",
                    "Bearer CALLER-BROWSER-AUTHORIZATION",
                ),
                ("cookie", "better-auth.session_token=CALLER-COOKIE"),
                ("x-org-id", "CALLER-ORG"),
                ("x-user-id", "CALLER-USER"),
                ("x-verevon-org-id", "CALLER-VEREVON-ORG"),
                ("x-service-id", "CALLER-SERVICE"),
            ],
            Some(start_body()),
        )
        .await;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["data"]["run_id"], "run-1");
        assert_eq!(body["data"]["allowed_domains"], json!(["example.com"]));
        assert_eq!(body["data"]["expires_at"], "2026-11-03T12:10:00Z");
        // The broker granted less than was asked: the tighter budget wins.
        assert_eq!(
            body["data"]["budget"],
            json!({"micro_usd": 500_000, "max_steps": 20, "max_runtime_s": 300})
        );
        assert!(
            body["data"].get("grant_id").is_none(),
            "the caller never sees a grant"
        );

        let requests = harness.all_upstream_requests().await;
        assert_eq!(requests.len(), 4, "two exchanges, one grant, one start");
        for (server, request) in &requests {
            for (name, value) in request.headers.iter() {
                let value = value.to_str().unwrap_or_default();
                assert!(
                    !value.contains("CALLER-"),
                    "{server}: caller header {name} leaked"
                );
            }
            for forbidden in ["cookie", "x-org-id", "x-user-id", API_KEY_HEADER] {
                assert!(
                    request.headers.get(forbidden).is_none(),
                    "{server}: {forbidden} forwarded"
                );
            }
            let body = String::from_utf8_lossy(&request.body);
            if server == "auth" {
                assert!(
                    body.contains(KEY),
                    "auth-core gets the key in the exchange body"
                );
                assert!(body.contains("\"orgId\":\"org-1\""));
                assert!(body.contains("browser:agent"));
            } else {
                assert!(
                    !body.contains(KEY),
                    "{server}: the raw key left the gateway"
                );
            }
        }
        let start = requests
            .iter()
            .find(|(server, request)| server == "quarry" && request.url.path() == "/v1/agent/runs")
            .map(|(_, request)| serde_json::from_slice::<Value>(&request.body).unwrap())
            .expect("quarry start");
        assert_eq!(start["grant_id"], "g-1");
        assert_eq!(start["initiated_by"], "agent");
        assert_eq!(start["zdr"], false);
        assert_eq!(start["constraints"]["max_steps"], 20);
        assert_eq!(
            start["constraints"]["allowed_domains"],
            json!(["example.com"])
        );
        assert!(start.get("profile_id").is_none());
        assert!(start.get("resume_run_id").is_none());
    }

    #[tokio::test]
    async fn a_zdr_grant_omits_hosts_and_marks_the_run_zdr() {
        let harness = Harness::start().await;
        harness
            .mount_exchange("browser-broker", "broker-token", 1)
            .await;
        harness.mount_exchange("quarry", "quarry-token", 1).await;
        harness.mount_grant("z_g-1", false).await;
        harness
            .mount_quarry_start(201, json!({"data": {"run_id": "run-z"}}))
            .await;
        let app = harness.app(config(600));
        let (status, _, body) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["data"]["zdr"], true);
        assert!(body["data"].get("allowed_domains").is_none(), "{body}");
        let started = harness.quarry.received_requests().await.unwrap();
        let start: Value = serde_json::from_slice(&started[0].body).unwrap();
        assert_eq!(start["zdr"], true, "a ZDR grant must back a ZDR run");
    }

    #[tokio::test]
    async fn an_exchange_refusal_keeps_its_reason_and_acquires_nothing() {
        let harness = Harness::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/browser-broker/api-key-exchange"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "code": "api_key_org_mismatch",
                "message": "API key exchange refused",
            })))
            .mount(&harness.auth)
            .await;
        let app = harness.app(config(600));
        let (status, _, body) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "api_key_refused");
        assert_eq!(body["error"]["details"]["reason"], "api_key_org_mismatch");
        assert!(harness
            .model_gateway
            .received_requests()
            .await
            .unwrap()
            .is_empty());
        assert!(harness.quarry.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_dark_exchange_route_is_503() {
        let harness = Harness::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/browser-broker/api-key-exchange"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "code": "api_key_agent_exchange_disabled"
            })))
            .mount(&harness.auth)
            .await;
        let app = harness.app(config(600));
        let (status, _, body) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 503);
        assert_eq!(body["error"]["details"]["reason"], "exchange_disabled");
    }

    #[tokio::test]
    async fn a_failed_quarry_start_revokes_the_grant() {
        let harness = Harness::start().await;
        harness
            .mount_exchange("browser-broker", "broker-token", 1)
            .await;
        harness.mount_exchange("quarry", "quarry-token", 1).await;
        harness.mount_grant("g-1", true).await;
        harness.mount_revoke("g-1", 1).await;
        harness
            .mount_quarry_start(
                403,
                json!({"error": {
                    "code": "forbidden",
                    "message": "internal detail http://10.0.0.1",
                    "details": {"reason": "api_key_grant_owner_mismatch"}
                }}),
            )
            .await;
        let app = harness.app(config(600));
        let (status, _, body) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "agent_run_refused");
        assert_eq!(
            body["error"]["details"]["reason"],
            "api_key_grant_owner_mismatch"
        );
        assert!(
            !body.to_string().contains("10.0.0.1"),
            "upstream messages never pass through: {body}"
        );
        // `expect(1)` on the revoke mock is verified when the harness drops.
    }

    #[tokio::test]
    async fn step_reuses_the_cached_token_and_forwards_the_body() {
        let harness = Harness::start().await;
        harness.mount_happy().await;
        harness
            .mount_quarry_step(
                "run-1",
                200,
                json!({"data": {"observation": {"title": "Example"}}}),
            )
            .await;
        let app = harness.app(config(600));
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 201);
        let step = json!({"action": {"type": "click", "selector": "#go"}});
        let (status, _, body) = call(
            &app,
            "POST",
            &format!("{RUNS}/run-1/step"),
            Some(KEY),
            &[("authorization", "Bearer CALLER-AUTHORIZATION")],
            Some(step.clone()),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["observation"]["title"], "Example");
        let steps: Vec<_> = harness
            .quarry
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/v1/agent/runs/run-1/step")
            .collect();
        assert_eq!(steps.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&steps[0].body).unwrap(),
            step
        );
        assert_eq!(
            steps[0].headers.get("authorization").unwrap(),
            "Bearer quarry-token"
        );
        // The quarry exchange mock expects exactly one call: the step used the cache.
    }

    #[tokio::test]
    async fn another_key_cannot_step_or_close_the_run() {
        let harness = Harness::start().await;
        harness.mount_happy().await;
        let app = harness.app(config(600));
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 201);
        let before = harness.all_upstream_requests().await.len();
        let (status, _, body) = call(
            &app,
            "POST",
            &format!("{RUNS}/run-1/step"),
            Some(OTHER_KEY),
            &[],
            Some(json!({"action": {"type": "noop"}})),
        )
        .await;
        assert_eq!(status, 404, "{body}");
        let (status, _, _) = call(
            &app,
            "DELETE",
            &format!("{RUNS}/run-1"),
            Some(OTHER_KEY),
            &[],
            None,
        )
        .await;
        assert_eq!(status, 404);
        let (status, _, _) = call(
            &app,
            "DELETE",
            &format!("{RUNS}/unknown-run"),
            Some(KEY),
            &[],
            None,
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(
            harness.all_upstream_requests().await.len(),
            before,
            "a refused step or close reaches no upstream"
        );
    }

    #[tokio::test]
    async fn close_deletes_the_run_and_revokes_the_grant() {
        let harness = Harness::start().await;
        harness.mount_happy().await;
        harness.mount_quarry_close("run-1").await;
        harness.mount_revoke("g-1", 1).await;
        let app = harness.app(config(600));
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 201);
        let (status, _, body) = call(
            &app,
            "DELETE",
            &format!("{RUNS}/run-1"),
            Some(KEY),
            &[],
            None,
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["closed"], true);
        // A second close finds no binding.
        let (status, _, _) = call(
            &app,
            "DELETE",
            &format!("{RUNS}/run-1"),
            Some(KEY),
            &[],
            None,
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn a_run_gone_upstream_revokes_the_grant_on_step() {
        let harness = Harness::start().await;
        harness.mount_happy().await;
        harness
            .mount_quarry_step(
                "run-1",
                404,
                json!({"error": {"code": "not_found", "message": "run not found"}}),
            )
            .await;
        harness.mount_revoke("g-1", 1).await;
        let app = harness.app(config(600));
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 201);
        let noop = json!({"action": {"type": "noop"}});
        let step_uri = format!("{RUNS}/run-1/step");
        let (status, _, _) =
            call(&app, "POST", &step_uri, Some(KEY), &[], Some(noop.clone())).await;
        assert_eq!(status, 404);
        let (status, _, _) = call(&app, "POST", &step_uri, Some(KEY), &[], Some(noop)).await;
        assert_eq!(status, 404, "the binding is gone too");
    }

    #[tokio::test]
    async fn the_ingress_rate_limit_is_per_key() {
        let harness = Harness::start().await;
        // 60 per hour = 1 per minute, a burst of one.
        let app = harness.app(config(60));
        let bad = json!({"org_id": ""});
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(bad.clone())).await;
        assert_eq!(status, 400);
        let (status, headers, body) =
            call(&app, "POST", RUNS, Some(KEY), &[], Some(bad.clone())).await;
        assert_eq!(status, 429, "{body}");
        assert_eq!(body["error"]["details"]["reason"], "api_key_rate_limited");
        assert!(headers.get(RETRY_AFTER).is_some());
        let (status, _, _) = call(&app, "POST", RUNS, Some(OTHER_KEY), &[], Some(bad)).await;
        assert_eq!(status, 400, "another key has its own bucket");
    }

    #[test]
    fn the_token_cache_key_never_contains_the_key() {
        let hash = key_hash(KEY);
        let cache_key = token_cache_key(&hash, "org-1", KeyAudience::Quarry);
        assert!(!cache_key.contains(KEY));
        assert_eq!(hash.len(), 64);
        assert_ne!(key_hash(KEY), key_hash(OTHER_KEY));
        assert!(!key_ref(&hash).contains(KEY));
    }

    #[test]
    fn upstream_reasons_are_tokens_never_free_text() {
        assert_eq!(
            upstream_reason(&json!({"code": "api_key_expired", "message": "x"})).as_deref(),
            Some("api_key_expired")
        );
        assert_eq!(
            upstream_reason(&json!({"error": {"code": "Bad thing at http://10.0.0.1"}})),
            None
        );
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_key_and_tokens_are_never_logged() {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let harness = Harness::start().await;
        harness
            .mount_exchange("browser-broker", "broker-token", 1)
            .await;
        harness.mount_exchange("quarry", "quarry-token", 1).await;
        harness.mount_grant("g-1", true).await;
        harness.mount_revoke("g-1", 1).await;
        harness
            .mount_quarry_start(500, json!({"error": {"code": "internal"}}))
            .await;
        let app = harness.app(config(600));
        let (status, _, _) = call(&app, "POST", RUNS, Some(KEY), &[], Some(start_body())).await;
        assert_eq!(status, 502);

        let bytes = buffer.0.lock().unwrap().clone();
        let logged = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            logged.contains("api-key agent run start refused"),
            "the refusal is logged: {logged}"
        );
        assert!(
            logged.contains(key_ref(&key_hash(KEY))),
            "logs carry the key ref"
        );
        for secret in [KEY, SERVICE_KEY, "broker-token", "quarry-token"] {
            assert!(
                !logged.contains(secret),
                "{secret} leaked into logs: {logged}"
            );
        }
        assert!(
            !logged.contains("example.com"),
            "hosts are never logged: {logged}"
        );
    }
}
