//! Short-lived, tenant-scoped service bearer acquisition from Auth Core.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

const SERVICE_ID: &str = "quarry-edge";
const DEFAULT_REFRESH_SKEW: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CACHE_ENTRIES: usize = 10_000;

#[derive(Clone)]
pub struct ServiceBearer(Arc<str>);

impl ServiceBearer {
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn from_test_token(token: impl Into<String>) -> Self {
        Self(Arc::from(token.into()))
    }
}

impl fmt::Debug for ServiceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceBearer(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceTokenAudience {
    DataPlane,
    ModelPlane,
}

impl ServiceTokenAudience {
    fn endpoint(self) -> &'static str {
        match self {
            Self::DataPlane => "/api/data-plane/internal-token",
            Self::ModelPlane => "/api/model-plane/internal-token",
        }
    }

    /// The name Auth Core's service-principal registry (`audiences`) and the
    /// minted token (`aud`) give this audience. The manifest names audiences
    /// this way, never by Rust variant.
    fn registry_audience(self) -> &'static str {
        match self {
            Self::DataPlane => "data-plane",
            Self::ModelPlane => "model-gateway",
        }
    }

    fn from_registry_audience(audience: &str) -> Option<Self> {
        [Self::DataPlane, Self::ModelPlane]
            .into_iter()
            .find(|candidate| candidate.registry_audience() == audience)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTokenRequest {
    audience: ServiceTokenAudience,
    scopes: Vec<String>,
    reason: String,
}

impl ServiceTokenRequest {
    /// The only way production code builds a token request: by the name of a
    /// row of `contracts/cross-plane-token-requests.json`.
    ///
    /// There is no public constructor from scopes, so a call site cannot widen
    /// its own request, and the Auth Core contract spec, which reads the same
    /// file, sees every request quarry-edge can make. `granted: false` rows
    /// are built too: whether the registry honours them is Auth Core's answer,
    /// and refusing locally would only hide that answer.
    ///
    /// Call it with a string literal: the manifest tests find call sites by
    /// scanning for `from_manifest("<row>")`.
    pub fn from_manifest(name: &str) -> QuarryResult<Self> {
        token_request_manifest()?
            .row(name)
            .ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!(
                        "service-token request '{name}' is not a row of the cross-plane \
                         token-request manifest"
                    ),
                )
            })?
            .request()
    }

    pub fn audience(&self) -> ServiceTokenAudience {
        self.audience
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[cfg(test)]
    fn data_plane<I, S>(scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(ServiceTokenAudience::DataPlane, scopes, reason)
    }

    #[cfg(test)]
    fn model_plane<I, S>(scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::new(ServiceTokenAudience::ModelPlane, scopes, reason)
    }

    fn new<I, S>(audience: ServiceTokenAudience, scopes: I, reason: impl Into<String>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut scopes: Vec<String> = scopes
            .into_iter()
            .map(Into::into)
            .map(|scope| scope.trim().to_owned())
            .filter(|scope| !scope.is_empty())
            .collect();
        scopes.sort();
        scopes.dedup();
        Self {
            audience,
            scopes,
            reason: reason.into().trim().to_owned(),
        }
    }

    fn validate(&self) -> QuarryResult<()> {
        if self.scopes.is_empty() || !(3..=500).contains(&self.reason.len()) {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "invalid bounded service-token request",
            ));
        }
        Ok(())
    }
}

/// The cross-plane token-request manifest, embedded at build time.
///
/// It lives inside this crate (`contracts/`) so it is inside the Docker build
/// context of every image that builds the workspace: the images copy `crates/`
/// and nothing above it, and a file kept elsewhere would compile on a
/// developer's machine and fail in the image.
pub const TOKEN_REQUEST_MANIFEST_JSON: &str =
    include_str!("../contracts/cross-plane-token-requests.json");

const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// One row of the manifest: a single request quarry-edge may make to Auth Core.
/// The manifest's own `description` says what `granted` and `pending` mean.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRequestRow {
    pub name: String,
    /// The registry audience: `data-plane` or `model-gateway`.
    pub audience: String,
    /// The Auth Core path that mints for that audience.
    pub mint_path: String,
    pub scopes: Vec<String>,
    pub reason: String,
    /// `true`: the registry must allow exactly this request. `false`: it must
    /// refuse it, and every caller of the row degrades.
    pub granted: bool,
    /// Workspace-relative files that request this row.
    pub used_by: Vec<String>,
    /// Set while the row has no call site yet: who adds it.
    #[serde(default)]
    pub pending: Option<String>,
    /// Environment switches that make the request reachable.
    pub feature_flags: Vec<String>,
    pub notes: String,
}

impl TokenRequestRow {
    /// The request this row stands for, in the form it is minted and cached.
    pub fn request(&self) -> QuarryResult<ServiceTokenRequest> {
        let audience =
            ServiceTokenAudience::from_registry_audience(&self.audience).ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!(
                        "token-request row '{}' names an unknown audience",
                        self.name
                    ),
                )
            })?;
        let request = ServiceTokenRequest::new(
            audience,
            self.scopes.iter().map(String::as_str),
            self.reason.as_str(),
        );
        request.validate()?;
        Ok(request)
    }

    /// What Auth Core would refuse for form alone, and what would make the
    /// request differ from the row: the row must already be in the form the
    /// constructor produces (trimmed, sorted, unique scopes; trimmed reason).
    fn check(&self) -> Result<(), String> {
        let audience =
            ServiceTokenAudience::from_registry_audience(&self.audience).ok_or_else(|| {
                format!(
                    "row {}: audience {:?} is not one quarry-edge mints for",
                    self.name, self.audience
                )
            })?;
        if self.mint_path != audience.endpoint() {
            return Err(format!(
                "row {}: mint_path {:?} is not the path that mints for {}",
                self.name, self.mint_path, self.audience
            ));
        }
        let request = ServiceTokenRequest::new(
            audience,
            self.scopes.iter().map(String::as_str),
            self.reason.as_str(),
        );
        if request.scopes != self.scopes || request.reason != self.reason {
            return Err(format!(
                "row {}: scopes must be trimmed, sorted and unique, and the reason trimmed",
                self.name
            ));
        }
        request.validate().map_err(|_| {
            format!(
                "row {}: needs at least one scope and a reason of 3..=500 characters",
                self.name
            )
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRequestManifest {
    pub schema_version: u32,
    pub service_id: String,
    pub description: String,
    pub rows: Vec<TokenRequestRow>,
}

impl TokenRequestManifest {
    fn parse(json: &str) -> Result<Self, String> {
        let manifest: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(format!(
                "schema_version {} is not the supported {MANIFEST_SCHEMA_VERSION}",
                manifest.schema_version
            ));
        }
        // The service id is also the `x-service-id` the provider sends, so the
        // rows and the header cannot name different principals.
        if manifest.service_id != SERVICE_ID {
            return Err(format!(
                "service_id {:?} is not {SERVICE_ID:?}",
                manifest.service_id
            ));
        }
        if manifest.rows.is_empty() {
            return Err("the manifest has no rows".to_owned());
        }
        let mut names = HashSet::new();
        for row in &manifest.rows {
            if !names.insert(row.name.as_str()) {
                return Err(format!("row {} is listed twice", row.name));
            }
            row.check()?;
        }
        Ok(manifest)
    }

    pub fn rows(&self) -> &[TokenRequestRow] {
        &self.rows
    }

    pub fn row(&self, name: &str) -> Option<&TokenRequestRow> {
        self.rows.iter().find(|row| row.name == name)
    }
}

/// The parsed, validated manifest. Parsed once; an invalid embedded file makes
/// every `from_manifest` fail with `Internal` rather than mint a request nobody
/// reviewed. The tests parse the shipped file, so it cannot be invalid in a
/// build that passed them.
pub fn token_request_manifest() -> QuarryResult<&'static TokenRequestManifest> {
    static PARSED: OnceLock<Result<TokenRequestManifest, String>> = OnceLock::new();
    PARSED
        .get_or_init(|| TokenRequestManifest::parse(TOKEN_REQUEST_MANIFEST_JSON))
        .as_ref()
        .map_err(|reason| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("cross-plane token-request manifest is invalid: {reason}"),
            )
        })
}

#[async_trait]
pub trait ServiceTokenProvider: Send + Sync {
    async fn token_for_org(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<ServiceBearer>;

    async fn invalidate(&self, org_id: &str, request: &ServiceTokenRequest);
}

pub type SharedServiceTokenProvider = Arc<dyn ServiceTokenProvider>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    audience: ServiceTokenAudience,
    org_id: String,
    scopes: Vec<String>,
}

struct CachedToken {
    bearer: ServiceBearer,
    refresh_at: Instant,
}

pub struct AuthCoreServiceTokenProvider {
    http: Client,
    auth_core_url: String,
    service_api_key: Arc<str>,
    refresh_skew: Duration,
    cache: tokio::sync::RwLock<HashMap<CacheKey, CachedToken>>,
    in_flight: dashmap::DashMap<CacheKey, Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MintRequest<'a> {
    org_id: &'a str,
    scopes: &'a [String],
    reason: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MintResponse {
    token: String,
    expires_in_seconds: u64,
}

impl AuthCoreServiceTokenProvider {
    pub fn new(
        auth_core_url: impl Into<String>,
        service_api_key: impl Into<String>,
    ) -> QuarryResult<Self> {
        let auth_core_url = auth_core_url.into().trim().trim_end_matches('/').to_owned();
        let service_api_key = service_api_key.into();
        if auth_core_url.is_empty() || reqwest::Url::parse(&auth_core_url).is_err() {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "AUTH_CORE_URL must be an absolute URL",
            ));
        }
        if service_api_key.len() < 16 {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "QUARRY_SERVICE_API_KEY must contain at least 16 characters",
            ));
        }
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // A redirect could move the durable service credential to a
            // different origin. Auth Core endpoints are canonical and must
            // return their token response directly.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("Auth Core token client initialization failed: {error}"),
                )
            })?;
        Ok(Self {
            http,
            auth_core_url,
            service_api_key: Arc::from(service_api_key),
            refresh_skew: DEFAULT_REFRESH_SKEW,
            cache: tokio::sync::RwLock::new(HashMap::new()),
            in_flight: dashmap::DashMap::new(),
        })
    }

    fn cache_key(org_id: &str, request: &ServiceTokenRequest) -> CacheKey {
        CacheKey {
            audience: request.audience,
            org_id: org_id.to_owned(),
            scopes: request.scopes.clone(),
        }
    }

    async fn mint(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
    ) -> QuarryResult<MintResponse> {
        let url = format!("{}{}", self.auth_core_url, request.audience.endpoint());
        let response = self
            .http
            .post(url)
            .header("x-service-id", SERVICE_ID)
            .header("x-service-api-key", self.service_api_key.as_ref())
            .json(&MintRequest {
                org_id,
                scopes: &request.scopes,
                reason: &request.reason,
            })
            .send()
            .await
            .map_err(|error| {
                let code = if error.is_timeout() {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::UpstreamBlocked
                };
                QuarryError::new(code, "Auth Core service-token request failed")
            })?;
        let status = response.status();
        if !status.is_success() {
            let code = match status.as_u16() {
                401 | 403 => ErrorCode::Forbidden,
                429 => ErrorCode::RateLimited,
                _ => ErrorCode::DriverFailed,
            };
            return Err(QuarryError::new(
                code,
                format!("Auth Core service-token request returned {status}"),
            ));
        }
        let minted = response.json::<MintResponse>().await.map_err(|_| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "Auth Core returned an invalid service-token response",
            )
        })?;
        if minted.token.trim().is_empty() || minted.expires_in_seconds == 0 {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "Auth Core returned an unusable service-token response",
            ));
        }
        Ok(minted)
    }
}

#[async_trait]
impl ServiceTokenProvider for AuthCoreServiceTokenProvider {
    async fn token_for_org(
        &self,
        org_id: &str,
        request: &ServiceTokenRequest,
        force_refresh: bool,
    ) -> QuarryResult<ServiceBearer> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "verified org_id is required for cross-plane authentication",
            ));
        }
        request.validate()?;
        let key = Self::cache_key(org_id, request);
        if !force_refresh {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if Instant::now() < cached.refresh_at {
                    return Ok(cached.bearer.clone());
                }
            }
        }

        // Singleflight is per tenant/audience/scope key. A slow mint for one
        // organization never blocks cached reads or unrelated tenants.
        let mint_lock = self
            .in_flight
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let mint_guard = mint_lock.lock().await;
        if !force_refresh {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if Instant::now() < cached.refresh_at {
                    drop(mint_guard);
                    self.remove_unused_mint_lock(&key, &mint_lock);
                    return Ok(cached.bearer.clone());
                }
            }
        }
        let minted = match self.mint(org_id, request).await {
            Ok(minted) => minted,
            Err(error) => {
                drop(mint_guard);
                self.remove_unused_mint_lock(&key, &mint_lock);
                return Err(error);
            }
        };
        let refresh_after =
            Duration::from_secs(minted.expires_in_seconds).saturating_sub(self.refresh_skew);
        let bearer = ServiceBearer(Arc::from(minted.token));
        let mut cache = self.cache.write().await;
        cache.retain(|_, cached| Instant::now() < cached.refresh_at);
        if cache.len() >= MAX_CACHE_ENTRIES {
            let to_drop: Vec<CacheKey> =
                cache.keys().take(MAX_CACHE_ENTRIES / 4).cloned().collect();
            for expired_key in to_drop {
                cache.remove(&expired_key);
            }
        }
        cache.insert(
            key,
            CachedToken {
                bearer: bearer.clone(),
                refresh_at: Instant::now() + refresh_after,
            },
        );
        drop(cache);
        drop(mint_guard);
        self.remove_unused_mint_lock(&Self::cache_key(org_id, request), &mint_lock);
        Ok(bearer)
    }

    async fn invalidate(&self, org_id: &str, request: &ServiceTokenRequest) {
        self.cache
            .write()
            .await
            .remove(&Self::cache_key(org_id.trim(), request));
    }
}

impl AuthCoreServiceTokenProvider {
    fn remove_unused_mint_lock(&self, key: &CacheKey, expected: &Arc<tokio::sync::Mutex<()>>) {
        if Arc::strong_count(expected) == 2 {
            self.in_flight
                .remove_if(key, |_, current| Arc::ptr_eq(current, expected));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn mints_and_caches_an_org_scoped_data_plane_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/data-plane/internal-token"))
            .and(header("x-service-id", "quarry-edge"))
            .and(header("x-service-api-key", "credential-long-enough"))
            .and(body_json(json!({
                "orgId": "org_verified",
                "scopes": ["documents:write"],
                "reason": "persist verified Quarry evidence"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "header.payload.signature",
                "expiresAt": "2099-01-01T00:00:00.000Z",
                "expiresInSeconds": 300,
                "issuer": "auth-core",
                "audience": "data-plane"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        let first = provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();
        let second = provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();

        assert_eq!(first.expose(), "header.payload.signature");
        assert_eq!(second.expose(), first.expose());
        assert_eq!(format!("{first:?}"), "ServiceBearer(<redacted>)");
    }

    #[tokio::test]
    async fn forced_refresh_mints_a_replacement_model_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/model-plane/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "header.payload.signature",
                "expiresInSeconds": 300
            })))
            .expect(2)
            .mount(&server)
            .await;
        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request =
            ServiceTokenRequest::model_plane(["models:invoke"], "invoke bounded model primitive");

        provider
            .token_for_org("org_verified", &request, false)
            .await
            .unwrap();
        provider
            .token_for_org("org_verified", &request, true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn refuses_unverified_empty_org_before_network_io() {
        let server = MockServer::start().await;
        let provider =
            AuthCoreServiceTokenProvider::new(server.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        let error = provider
            .token_for_org("  ", &request, false)
            .await
            .unwrap_err();
        assert_eq!(error.code, quarry_core::error::ErrorCode::Forbidden);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn never_forwards_service_credentials_across_redirects() {
        let credential_sink = MockServer::start().await;
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/data-plane/internal-token"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/stolen", credential_sink.uri())),
            )
            .mount(&auth_core)
            .await;
        let provider =
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap();
        let request = ServiceTokenRequest::data_plane(
            ["documents:write"],
            "persist verified Quarry evidence",
        );

        assert!(provider
            .token_for_org("org_verified", &request, false)
            .await
            .is_err());
        assert!(credential_sink
            .received_requests()
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_short_service_credentials() {
        let result = AuthCoreServiceTokenProvider::new("http://auth-core:3011", "too-short");
        assert!(result.is_err());
    }
}

/// The manifest loader. The file itself (shape, scope rules, pin, call sites,
/// the request on the wire) is held to account by `token_request_contract.rs`,
/// which does not go through this loader; these tests are about the loader.
#[cfg(test)]
mod manifest_tests {
    use super::*;
    use serde_json::{json, Value};

    /// Every request a row must stand for, typed here and not read from the
    /// manifest: the first five are exactly what the call sites wrote inline
    /// before the manifest existed, so the refactor cannot change a request
    /// unnoticed, and editing a row means editing this table too.
    const EXPECTED_REQUESTS: [(&str, ServiceTokenAudience, &[&str], &str); 6] = [
        (
            "vector-retrieve",
            ServiceTokenAudience::DataPlane,
            &["data:read"],
            "retrieve verified organization evidence",
        ),
        (
            "ingest",
            ServiceTokenAudience::DataPlane,
            &["documents:write", "org:data:read_all"],
            "persist verified Quarry evidence",
        ),
        (
            "web-rerank",
            ServiceTokenAudience::DataPlane,
            &["data:rerank"],
            "rank web search candidates",
        ),
        (
            "model-invoke",
            ServiceTokenAudience::ModelPlane,
            &["models:invoke"],
            "invoke bounded model primitive",
        ),
        (
            "model-stream",
            ServiceTokenAudience::ModelPlane,
            &["models:invoke"],
            "stream bounded model primitive",
        ),
        (
            "model-audio",
            ServiceTokenAudience::ModelPlane,
            &["models:invoke"],
            "invoke bounded audio primitive",
        ),
    ];

    fn raw_manifest() -> Value {
        serde_json::from_str(TOKEN_REQUEST_MANIFEST_JSON).expect("the manifest is valid JSON")
    }

    fn parse_after(edit: impl FnOnce(&mut Value)) -> Result<TokenRequestManifest, String> {
        let mut raw = raw_manifest();
        edit(&mut raw);
        TokenRequestManifest::parse(&raw.to_string())
    }

    #[test]
    fn the_shipped_manifest_is_accepted_unchanged() {
        parse_after(|_| {}).expect("the shipped manifest loads");
        let manifest = token_request_manifest().expect("and so does the cached copy");
        let in_manifest: Vec<&str> = manifest
            .rows()
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        let expected: Vec<&str> = EXPECTED_REQUESTS.iter().map(|(name, ..)| *name).collect();
        assert_eq!(
            in_manifest, expected,
            "the manifest and EXPECTED_REQUESTS list different rows"
        );
    }

    #[test]
    fn from_manifest_builds_the_requests_the_call_sites_used_to_write_inline() {
        for (name, audience, scopes, reason) in EXPECTED_REQUESTS {
            let request = ServiceTokenRequest::from_manifest(name).expect(name);
            assert_eq!(request.audience(), audience, "{name}");
            assert_eq!(request.scopes(), scopes, "{name}");
            assert_eq!(request.reason(), reason, "{name}");
            assert_eq!(
                request,
                ServiceTokenRequest::new(audience, scopes.iter().copied(), reason),
                "{name}"
            );
        }
    }

    #[test]
    fn from_manifest_agrees_with_the_file_read_as_plain_json() {
        let raw = raw_manifest();
        for row in raw["rows"].as_array().expect("rows is an array") {
            let name = row["name"].as_str().expect("a row has a name");
            let request = ServiceTokenRequest::from_manifest(name).expect(name);
            let audience = match row["audience"].as_str().expect("an audience") {
                "data-plane" => ServiceTokenAudience::DataPlane,
                "model-gateway" => ServiceTokenAudience::ModelPlane,
                other => panic!("row {name}: unexpected audience {other}"),
            };
            assert_eq!(request.audience(), audience, "{name}");
            assert_eq!(
                request.audience().endpoint(),
                row["mint_path"].as_str().expect("a mint_path"),
                "{name}"
            );
            let scopes: Vec<&str> = row["scopes"]
                .as_array()
                .expect("scopes is an array")
                .iter()
                .map(|scope| scope.as_str().expect("a scope is a string"))
                .collect();
            assert_eq!(request.scopes(), scopes.as_slice(), "{name}");
            assert_eq!(
                request.reason(),
                row["reason"].as_str().expect("a reason"),
                "{name}"
            );
        }
    }

    #[test]
    fn a_name_that_is_not_a_row_cannot_be_built() {
        for name in ["", "nope", "Ingest", "ingest ", "web_rerank"] {
            let error = ServiceTokenRequest::from_manifest(name).unwrap_err();
            assert_eq!(error.code, ErrorCode::Internal, "{name:?}");
        }
    }

    #[test]
    fn a_manifest_that_auth_core_would_refuse_for_form_is_refused_at_load() {
        let cases: [(&str, fn(&mut Value)); 16] = [
            ("unsorted scopes", |raw| {
                raw["rows"][1]["scopes"] = json!(["org:data:read_all", "documents:write"]);
            }),
            ("a repeated scope", |raw| {
                raw["rows"][0]["scopes"] = json!(["data:read", "data:read"]);
            }),
            ("a padded scope", |raw| {
                raw["rows"][0]["scopes"] = json!(["data:read "]);
            }),
            ("no scope", |raw| {
                raw["rows"][0]["scopes"] = json!([]);
            }),
            ("a padded reason", |raw| {
                raw["rows"][0]["reason"] = json!(" retrieve verified organization evidence");
            }),
            ("a reason shorter than 3", |raw| {
                raw["rows"][0]["reason"] = json!("ab");
            }),
            ("a reason longer than 500", |raw| {
                raw["rows"][0]["reason"] = json!("x".repeat(501));
            }),
            ("an audience quarry-edge never mints for", |raw| {
                raw["rows"][0]["audience"] = json!("cost-core");
            }),
            ("a mint_path that is not its audience's", |raw| {
                raw["rows"][0]["mint_path"] = json!("/api/model-plane/internal-token");
            }),
            ("a repeated row name", |raw| {
                raw["rows"][1]["name"] = json!("vector-retrieve");
            }),
            ("an unknown row field", |raw| {
                raw["rows"][0]["surprise"] = json!(true);
            }),
            ("an unknown top-level field", |raw| {
                raw["surprise"] = json!(1);
            }),
            ("another schema version", |raw| {
                raw["schema_version"] = json!(2);
            }),
            ("another service id", |raw| {
                raw["service_id"] = json!("model-gateway");
            }),
            ("no rows", |raw| {
                raw["rows"] = json!([]);
            }),
            ("a row without `granted`", |raw| {
                raw["rows"][0].as_object_mut().unwrap().remove("granted");
            }),
        ];
        for (label, edit) in cases {
            assert!(
                parse_after(edit).is_err(),
                "a manifest with {label} was accepted"
            );
        }
    }
}
