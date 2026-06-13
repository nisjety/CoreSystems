//! Semantic-response cache for the model-gateway, behind one `SemanticCache`
//! seam with two interchangeable backends selected from the environment:
//!
//!   - **Managed Redis `LangCache`** (`LANGCACHE_URL` / `LANGCACHE_CACHE_ID` /
//!     `LANGCACHE_API_KEY`): a hosted REST service that generates embeddings
//!     server-side and matches *semantically* above a similarity threshold, so
//!     this client only ships prompt/response text plus scoping attributes
//!     (`org_id` + `model`).
//!   - **Local Dragonfly exact-match** (`SEMANTIC_CACHE_URL`): a boundary-safe
//!     KV cache keyed by the *exact* `(org_id, model, prompt)`. No embeddings and
//!     no cross-plane calls, so it honors the gateway's "does NOT embed a second
//!     vector store" invariant; the vector-similarity tier is owned by Data Plane
//!     v2 and layered on separately. A byte-identical prompt from the same
//!     org + model hits; anything else misses — strictly stricter (and so safer)
//!     than the semantic backend.
//!
//! `org_id` scoping keeps one tenant from reading another's cached responses.
//! Selection precedence: managed LangCache → local Dragonfly → disabled (every
//! `Invoke` hits inference-core), matching the dev-friendly "disabled when
//! unconfigured" pattern used elsewhere. Both backends are strictly best-effort:
//! any transport/parse error degrades to a miss so inference still runs.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const DEFAULT_THRESHOLD: f64 = 0.9;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default TTL for locally-cached responses (1 hour).
const DEFAULT_CACHE_TTL_SECS: u64 = 3600;
/// Key namespace for the local Dragonfly response cache.
const CACHE_KEY_PREFIX: &str = "mp:gw:cache:";

static GLOBAL: OnceLock<Option<SemanticCache>> = OnceLock::new();

/// Returns the process-global semantic cache, lazily initialized from the
/// environment. `None` means no cache is configured — callers proceed straight
/// to inference.
pub fn global() -> Option<&'static SemanticCache> {
    GLOBAL.get_or_init(SemanticCache::from_env).as_ref()
}

/// The active cache backend behind one `lookup`/`store` seam. Selection
/// precedence (first match wins): managed Redis `LangCache` → local Dragonfly
/// exact-match → disabled.
pub enum SemanticCache {
    /// Hosted Redis LangCache (server-side embeddings + similarity threshold).
    Managed(LangCacheClient),
    /// Data-Plane-v2-owned semantic (vector-similarity) cache, reached over HTTP.
    DataPlane(DataPlaneCache),
    /// Local Dragonfly exact-match KV (no embeddings, no cross-plane calls).
    Local(DragonflyCache),
}

impl SemanticCache {
    fn from_env() -> Option<Self> {
        if let Some(client) = LangCacheClient::from_env() {
            return Some(Self::Managed(client));
        }
        if let Some(cache) = DataPlaneCache::from_env() {
            return Some(Self::DataPlane(cache));
        }
        if let Some(cache) = DragonflyCache::from_env() {
            return Some(Self::Local(cache));
        }
        None
    }

    /// Look up a cached response for `prompt`, scoped to org + model. A miss or
    /// any error yields `None` so the caller falls through to inference.
    pub async fn lookup(&self, prompt: &str, org_id: &str, model: &str) -> Option<String> {
        match self {
            Self::Managed(c) => c.lookup(prompt, org_id, model).await,
            Self::DataPlane(c) => c.lookup(prompt, org_id, model).await,
            Self::Local(c) => c.lookup(prompt, org_id, model).await,
        }
    }

    /// Store a prompt/response pair for future hits. Best-effort; never panics.
    pub async fn store(&self, prompt: &str, org_id: &str, model: &str, response: &str) {
        match self {
            Self::Managed(c) => c.store(prompt, org_id, model, response).await,
            Self::DataPlane(c) => c.store(prompt, org_id, model, response).await,
            Self::Local(c) => c.store(prompt, org_id, model, response).await,
        }
    }
}

/// Local exact-match response cache backed by Dragonfly (Redis-wire protocol).
///
/// Keyed by the *exact* `(org_id, model, prompt)`. Enabled by `SEMANTIC_CACHE_URL`
/// (e.g. `redis://mp-dragonfly:6379`); TTL via `SEMANTIC_CACHE_TTL_SECS` (default
/// `3600`). The connection is established lazily on first use, mirroring
/// `stream_buffer.rs`'s `ConnectionManager` pattern.
pub struct DragonflyCache {
    client: redis::Client,
    conn: tokio::sync::OnceCell<redis::aio::ConnectionManager>,
    ttl_secs: u64,
}

impl DragonflyCache {
    /// Build from the environment. Returns `None` when `SEMANTIC_CACHE_URL` is
    /// unset/empty or not a valid Redis URL. Reusing the gateway's Dragonfly is
    /// opt-in (point `SEMANTIC_CACHE_URL` at the same instance as `REDIS_URL`) so
    /// a stream-buffer-only deployment never silently starts caching responses.
    pub fn from_env() -> Option<Self> {
        let url = non_empty_env("SEMANTIC_CACHE_URL")?;
        let client = redis::Client::open(url).ok()?;
        let ttl_secs = std::env::var("SEMANTIC_CACHE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|t| *t > 0)
            .unwrap_or(DEFAULT_CACHE_TTL_SECS);
        tracing::info!(
            ttl_secs,
            "semantic cache enabled (local Dragonfly exact-match)"
        );
        Some(Self {
            client,
            conn: tokio::sync::OnceCell::new(),
            ttl_secs,
        })
    }

    /// Lazily-established multiplexed connection; a connect failure degrades to a
    /// cache miss rather than propagating.
    async fn manager(&self) -> Option<redis::aio::ConnectionManager> {
        self.conn
            .get_or_try_init(|| redis::aio::ConnectionManager::new(self.client.clone()))
            .await
            .map_err(|error| tracing::debug!(%error, "semantic cache connect failed"))
            .ok()
            .cloned()
    }

    /// Cache key: `org_id` + `model` are exact path segments (so they never
    /// collide across tenants/models); only the (potentially large) prompt is
    /// hashed, with its byte length appended as a cheap second discriminator
    /// against the already-negligible 64-bit hash-collision chance.
    fn key(prompt: &str, org_id: &str, model: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        prompt.hash(&mut hasher);
        let digest = hasher.finish();
        format!(
            "{CACHE_KEY_PREFIX}{org_id}:{model}:{digest:016x}:{}",
            prompt.len()
        )
    }

    async fn lookup(&self, prompt: &str, org_id: &str, model: &str) -> Option<String> {
        use redis::AsyncCommands;
        let mut conn = self.manager().await?;
        let key = Self::key(prompt, org_id, model);
        match conn.get::<_, Option<String>>(&key).await {
            Ok(value) => value.filter(|v| !v.is_empty()),
            Err(error) => {
                tracing::debug!(%error, "semantic cache get failed");
                None
            }
        }
    }

    async fn store(&self, prompt: &str, org_id: &str, model: &str, response: &str) {
        if response.is_empty() {
            return;
        }
        use redis::AsyncCommands;
        let Some(mut conn) = self.manager().await else {
            return;
        };
        let key = Self::key(prompt, org_id, model);
        let result: redis::RedisResult<()> = conn.set_ex(&key, response, self.ttl_secs).await;
        if let Err(error) = result {
            tracing::debug!(%error, "semantic cache set failed");
        }
    }
}

/// Data-Plane-v2-owned semantic (vector-similarity) cache. The gateway cannot
/// host a vector store (see `retrieval.rs`), so the semantic tier lives in Data
/// Plane v2 (embeddings + Qdrant) and the gateway calls it over HTTP. Enabled by
/// `SEMANTIC_CACHE_DATAPLANE_ENABLED=true`; reuses the Data Plane retrieval HTTP
/// base (`DATAPLANE_RETRIEVAL_HTTP_URL`) + `DATAPLANE_INTERNAL_KEY` the gateway
/// already uses for graph grounding. Best-effort: any error degrades to a miss.
pub struct DataPlaneCache {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl DataPlaneCache {
    /// Build from the environment. Returns `None` unless
    /// `SEMANTIC_CACHE_DATAPLANE_ENABLED` is truthy, so the slower vector tier is
    /// strictly opt-in (the gateway otherwise uses the Dragonfly exact-match).
    pub fn from_env() -> Option<Self> {
        if !env_flag("SEMANTIC_CACHE_DATAPLANE_ENABLED") {
            return None;
        }
        let base_url = std::env::var("DATAPLANE_RETRIEVAL_HTTP_URL")
            .or_else(|_| std::env::var("DATA_PLANE_RETRIEVAL_URL"))
            .unwrap_or_else(|_| "http://dpv2-retrieval-engine:8004".to_owned())
            .trim_end_matches('/')
            .to_owned();
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .ok()?;
        tracing::info!("semantic cache enabled (Data Plane v2 vector tier)");
        Some(Self {
            http,
            base_url,
            api_key: non_empty_env("DATAPLANE_INTERNAL_KEY"),
        })
    }

    async fn lookup(&self, prompt: &str, org_id: &str, model: &str) -> Option<String> {
        let url = format!("{}/v1/cache/semantic/search", self.base_url);
        let payload = serde_json::json!({ "org_id": org_id, "model": model, "prompt": prompt });
        let bytes = serde_json::to_vec(&payload).ok()?;
        let mut request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(bytes);
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        let resp = request.send().await.ok()?;
        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "semantic cache (data plane) search non-success");
            return None;
        }
        let raw = resp.bytes().await.ok()?;
        let body: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        if body.get("hit").and_then(serde_json::Value::as_bool) != Some(true) {
            return None;
        }
        body.get("response")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    async fn store(&self, prompt: &str, org_id: &str, model: &str, response: &str) {
        if response.is_empty() {
            return;
        }
        let url = format!("{}/v1/cache/semantic/store", self.base_url);
        let payload = serde_json::json!({
            "org_id": org_id,
            "model": model,
            "prompt": prompt,
            "response": response,
        });
        let Ok(bytes) = serde_json::to_vec(&payload) else {
            return;
        };
        let mut request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .body(bytes);
        if let Some(key) = &self.api_key {
            request = request.header("x-api-key", key);
        }
        if let Err(error) = request.send().await {
            tracing::debug!(%error, "semantic cache (data plane) store failed");
        }
    }
}

#[derive(Clone)]
pub struct LangCacheClient {
    http: reqwest::Client,
    base_url: String,
    cache_id: String,
    api_key: String,
    threshold: f64,
}

#[derive(Serialize)]
struct SearchRequest<'a> {
    prompt: &'a str,
    #[serde(rename = "similarityThreshold")]
    similarity_threshold: f64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    attributes: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
struct StoreRequest<'a> {
    prompt: &'a str,
    response: &'a str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    attributes: BTreeMap<&'static str, String>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    response: String,
}

impl LangCacheClient {
    /// Build from the environment. Returns `None` when any required variable is
    /// missing or empty.
    pub fn from_env() -> Option<Self> {
        let base_url = non_empty_env("LANGCACHE_URL")?;
        let cache_id = non_empty_env("LANGCACHE_CACHE_ID")?;
        let api_key = non_empty_env("LANGCACHE_API_KEY")?;
        let threshold = std::env::var("LANGCACHE_THRESHOLD")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|t| *t > 0.0)
            .unwrap_or(DEFAULT_THRESHOLD);
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .ok()?;
        tracing::info!("langcache enabled");
        Some(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            cache_id,
            api_key,
            threshold,
        })
    }

    fn attrs(org_id: &str, model: &str) -> BTreeMap<&'static str, String> {
        let mut m = BTreeMap::new();
        if !org_id.is_empty() {
            m.insert("org_id", org_id.to_owned());
        }
        if !model.is_empty() {
            m.insert("model", model.to_owned());
        }
        m
    }

    /// Look up a cached response for `prompt`, scoped to org + model. Returns the
    /// cached text on a hit; any miss or error yields `None` so the caller falls
    /// through to inference.
    pub async fn lookup(&self, prompt: &str, org_id: &str, model: &str) -> Option<String> {
        let body = SearchRequest {
            prompt,
            similarity_threshold: self.threshold,
            attributes: Self::attrs(org_id, model),
        };
        let url = format!(
            "{}/v1/caches/{}/entries/search",
            self.base_url, self.cache_id
        );
        let bytes = serde_json::to_vec(&body).ok()?;
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(bytes)
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "langcache search non-success");
            return None;
        }
        let raw = resp.bytes().await.ok()?;
        parse_entries(&raw)?
            .into_iter()
            .map(|e| e.response)
            .find(|r| !r.is_empty())
    }

    /// Store a prompt/response pair for future semantically-similar prompts.
    /// Best-effort: errors are logged, never propagated.
    pub async fn store(&self, prompt: &str, org_id: &str, model: &str, response: &str) {
        if response.is_empty() {
            return;
        }
        let body = StoreRequest {
            prompt,
            response,
            attributes: Self::attrs(org_id, model),
        };
        let url = format!("{}/v1/caches/{}/entries", self.base_url, self.cache_id);
        let Ok(bytes) = serde_json::to_vec(&body) else {
            return;
        };
        let result = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(bytes)
            .send()
            .await;
        if let Err(error) = result {
            tracing::debug!(%error, "langcache store failed");
        }
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// True when `key` is set to a truthy value (`1`/`true`/`yes`/`on`, any case).
fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Parse a search response that may be a bare JSON array of entries or a
/// `{"data": [...]}` envelope, tolerating either documented shape.
fn parse_entries(raw: &[u8]) -> Option<Vec<Entry>> {
    let first = *raw.iter().find(|&&b| !b.is_ascii_whitespace())?;
    match first {
        b'[' => serde_json::from_slice::<Vec<Entry>>(raw).ok(),
        b'{' => {
            #[derive(Deserialize)]
            struct Wrapper {
                #[serde(default)]
                data: Vec<Entry>,
            }
            serde_json::from_slice::<Wrapper>(raw).ok().map(|w| w.data)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client(base_url: String) -> LangCacheClient {
        LangCacheClient {
            http: reqwest::Client::new(),
            base_url,
            cache_id: "c1".to_owned(),
            api_key: "k".to_owned(),
            threshold: 0.9,
        }
    }

    #[test]
    fn parses_bare_array() {
        let entries = parse_entries(br#"[{"response":"hi"}]"#).unwrap();
        assert_eq!(entries[0].response, "hi");
    }

    #[test]
    fn parses_data_envelope() {
        let entries = parse_entries(br#"{"data":[{"response":"yo"}]}"#).unwrap();
        assert_eq!(entries[0].response, "yo");
    }

    #[test]
    fn empty_response_body_is_none() {
        assert!(parse_entries(b"   ").is_none());
    }

    #[test]
    fn cache_key_is_scoped_stable_and_collision_guarded() {
        let key = DragonflyCache::key("hello", "org-1", "m");
        assert_eq!(key, DragonflyCache::key("hello", "org-1", "m"), "stable");
        assert!(
            key.starts_with("mp:gw:cache:org-1:m:"),
            "namespaced by org + model"
        );
        assert_ne!(
            key,
            DragonflyCache::key("hello", "org-2", "m"),
            "org-scoped"
        );
        assert_ne!(
            key,
            DragonflyCache::key("hello", "org-1", "m2"),
            "model-scoped"
        );
        assert_ne!(
            key,
            DragonflyCache::key("HELLO", "org-1", "m"),
            "prompt is case-sensitive"
        );
        assert!(key.ends_with(":5"), "byte-length discriminator appended");
    }

    #[tokio::test]
    async fn lookup_returns_cached_response_on_hit() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/caches/c1/entries/search"))
            .and(header("Authorization", "Bearer k"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"[{"response":"cached answer"}]"#),
            )
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let hit = client.lookup("hello", "org-1", "model-x").await;
        assert_eq!(hit.as_deref(), Some("cached answer"));
    }

    #[tokio::test]
    async fn lookup_returns_none_on_miss() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/caches/c1/entries/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        assert!(client.lookup("hello", "org-1", "model-x").await.is_none());
    }

    #[tokio::test]
    async fn store_posts_to_entries_endpoint() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/caches/c1/entries"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        client.store("hello", "org-1", "model-x", "fresh").await;
        // MockServer verifies the expected POST on drop.
    }
}
