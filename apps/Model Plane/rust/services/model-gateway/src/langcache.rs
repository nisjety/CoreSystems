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
//! Selection precedence: managed `LangCache` → local Dragonfly → disabled (every
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

fn cache_io_allowed(zdr: bool) -> bool {
    !zdr
}

/// Whether a finished turn may be cached, and whether a cached answer may be
/// served for it.
///
/// # Staleness is not the reason — replay fidelity is
///
/// The obvious rule would be "never cache a turn that used live data". It is
/// also unnecessary here, and understanding why keeps the next reader from
/// tightening this into uselessness: the cache key is the FULLY ASSEMBLED
/// prompt, and a tool result or a retrieved passage is *in* that prompt. Change
/// the ERP number and the prompt changes, so the key changes, so the old answer
/// is unreachable. Nothing can go stale behind a key that contains it.
///
/// What a cached answer genuinely cannot reproduce is everything that was NOT
/// answer text. A grounded turn also emitted `citation` events; a tool turn also
/// emitted `tool_call` and `tool_result` events. This cache stores one string,
/// so replaying it would hand the user the same prose stripped of its sources
/// and its visible work — claims with no citations, which is worse than a
/// slower answer.
///
/// So the exclusions are:
///
///   * **turns that emitted citations** (knowledge-base or web) — the sources
///     would be lost;
///   * **tool turns** — the tool timeline would be lost;
///   * **structured-output turns**, whose schema lives on the request rather
///     than in the messages and therefore is NOT in the key. Two different
///     schemas over identical messages would collide. Excluded rather than
///     keyed, because such calls are machine one-offs nobody re-asks;
///   * **ZDR turns**, which must leave nothing behind at all.
///
/// Notably absent: context-assembly grounding. Assembled Data Plane context is
/// prompt text and emits no event of its own, so a text-only replay of it is
/// faithful — and being in the key, it cannot be stale either. Gating on it
/// instead made the cache store nothing at all in any deployment with Data
/// Plane wired up, which is how the distinction above was found.
///
/// What is left is the case this cache is for: a plain question, answered from
/// the model and the conversation, asked again. The prompt carries today's date
/// (the temporal-awareness system message), so entries fall out of reach on
/// their own even before the TTL.
// Four independent yes/no exclusions. An enum would force them to be mutually
// exclusive, and a turn can easily trip two at once.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TurnCacheability {
    pub zdr: bool,
    /// The turn emitted `tool_call`/`tool_result` events.
    pub used_tools: bool,
    /// The turn emitted `citation` events (knowledge-base or web).
    pub emitted_citations: bool,
    pub structured_output: bool,
}

impl TurnCacheability {
    /// True only when every exclusion above is clear.
    #[must_use]
    pub const fn is_cacheable(self) -> bool {
        !self.zdr && !self.used_tools && !self.emitted_citations && !self.structured_output
    }
}

/// Who a cached answer belongs to.
///
/// `user_id` is part of the identity, not a nice-to-have. The exact-match
/// backend is *incidentally* user-safe today — per-user memory is injected into
/// the prompt, so two users produce different keys — but that is a property of
/// what happens to be in the prompt rather than a guarantee, and it does not
/// hold at all for the similarity-matching backends: two colleagues in one org
/// asking near-identical questions match each other above the threshold, and
/// one of them gets an answer built from the other's memories.
///
/// Scoping by user makes the guarantee explicit and backend-independent. The
/// cost is a lower hit rate on genuinely shared questions, which is the correct
/// trade for a per-user assistant.
#[derive(Clone, Copy, Debug)]
pub struct CacheScope<'a> {
    pub org_id: &'a str,
    pub user_id: &'a str,
    pub model: &'a str,
}

impl CacheScope<'_> {
    /// The scope as ordered attributes for backends that filter rather than key.
    fn attributes(self) -> BTreeMap<&'static str, String> {
        let mut attributes = BTreeMap::new();
        if !self.org_id.is_empty() {
            attributes.insert("org_id", self.org_id.to_owned());
        }
        if !self.user_id.is_empty() {
            attributes.insert("user_id", self.user_id.to_owned());
        }
        if !self.model.is_empty() {
            attributes.insert("model", self.model.to_owned());
        }
        attributes
    }
}

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
    /// Hosted Redis `LangCache` (server-side embeddings + similarity threshold).
    Managed(LangCacheClient),
    /// Data-Plane-v2-owned semantic (vector-similarity) cache, reached over HTTP.
    DataPlane(DataPlaneCache),
    /// Local Dragonfly exact-match KV (no embeddings, no cross-plane calls).
    Local(DragonflyCache),
    /// Both local tiers layered: a fast Dragonfly exact-match in front of the
    /// Data Plane v2 semantic fallback. Reads hit exact first, then semantic
    /// (warming exact on the way back); writes populate both.
    Layered {
        exact: DragonflyCache,
        semantic: DataPlaneCache,
    },
}

impl SemanticCache {
    fn from_env() -> Option<Self> {
        // Hosted Redis LangCache wins outright when configured.
        if let Some(client) = LangCacheClient::from_env() {
            return Some(Self::Managed(client));
        }
        // Otherwise compose the local tiers: a fast Dragonfly exact-match in
        // front of the Data Plane v2 semantic fallback. Either alone is used solo.
        match (DragonflyCache::from_env(), DataPlaneCache::from_env()) {
            (Some(exact), Some(semantic)) => Some(Self::Layered { exact, semantic }),
            (Some(exact), None) => Some(Self::Local(exact)),
            (None, Some(semantic)) => Some(Self::DataPlane(semantic)),
            (None, None) => None,
        }
    }

    /// Look up a cached response for `prompt`, scoped to org + model. A miss or
    /// any error yields `None` so the caller falls through to inference.
    pub async fn lookup(&self, prompt: &str, scope: CacheScope<'_>, zdr: bool) -> Option<String> {
        if !cache_io_allowed(zdr) {
            return None;
        }
        match self {
            Self::Managed(c) => c.lookup(prompt, scope).await,
            Self::DataPlane(c) => c.lookup(prompt, scope).await,
            Self::Local(c) => c.lookup(prompt, scope).await,
            Self::Layered { exact, semantic } => {
                // Fast path: a local Dragonfly exact-match hit.
                if let Some(hit) = exact.lookup(prompt, scope).await {
                    return Some(hit);
                }
                // Fallback: a semantic (vector) hit from Data Plane v2. Warm the
                // exact tier so a repeat of this exact prompt stays a local hit.
                let hit = semantic.lookup(prompt, scope).await?;
                exact.store(prompt, scope, &hit).await;
                Some(hit)
            }
        }
    }

    /// Store a prompt/response pair for future hits. Best-effort; never panics.
    pub async fn store(&self, prompt: &str, scope: CacheScope<'_>, response: &str, zdr: bool) {
        if !cache_io_allowed(zdr) {
            return;
        }
        match self {
            Self::Managed(c) => c.store(prompt, scope, response).await,
            Self::DataPlane(c) => c.store(prompt, scope, response).await,
            Self::Local(c) => c.store(prompt, scope, response).await,
            Self::Layered { exact, semantic } => {
                // Populate both tiers so future exact AND near-duplicate prompts hit.
                exact.store(prompt, scope, response).await;
                semantic.store(prompt, scope, response).await;
            }
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

    /// Cache key: org, user and model are exact path segments (so they never
    /// collide across tenants, people or models); only the (potentially large)
    /// prompt is hashed, with its byte length appended as a cheap second
    /// discriminator against the already-negligible 64-bit hash-collision
    /// chance.
    ///
    /// The user segment is LENGTH-PREFIXED (`7#user-ab`) rather than written
    /// plain or replaced by a placeholder when blank. Any placeholder is a value
    /// some real user id could also be — a first attempt used `-`, and the test
    /// below immediately found that a user literally named `-` would then share
    /// a key with an unscoped one. A length prefix cannot collide.
    fn key(prompt: &str, scope: CacheScope<'_>) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        prompt.hash(&mut hasher);
        let digest = hasher.finish();
        format!(
            "{CACHE_KEY_PREFIX}{}:{}#{}:{}:{digest:016x}:{}",
            scope.org_id,
            scope.user_id.len(),
            scope.user_id,
            scope.model,
            prompt.len()
        )
    }

    async fn lookup(&self, prompt: &str, scope: CacheScope<'_>) -> Option<String> {
        use redis::AsyncCommands;
        let mut conn = self.manager().await?;
        let key = Self::key(prompt, scope);
        match conn.get::<_, Option<String>>(&key).await {
            Ok(value) => value.filter(|v| !v.is_empty()),
            Err(error) => {
                tracing::debug!(%error, "semantic cache get failed");
                None
            }
        }
    }

    async fn store(&self, prompt: &str, scope: CacheScope<'_>, response: &str) {
        use redis::AsyncCommands;
        if response.is_empty() {
            return;
        }
        let Some(mut conn) = self.manager().await else {
            return;
        };
        let key = Self::key(prompt, scope);
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
}

impl DataPlaneCache {
    /// Build from the environment. Returns `None` unless
    /// `SEMANTIC_CACHE_DATAPLANE_ENABLED` is truthy, so the slower vector tier is
    /// strictly opt-in (the gateway otherwise uses the Dragonfly exact-match).
    pub fn from_env() -> Option<Self> {
        if !env_flag("SEMANTIC_CACHE_DATAPLANE_ENABLED") {
            return None;
        }
        tracing::warn!(
            "Data Plane semantic cache disabled: a request-bound verified bearer is required"
        );
        None
    }

    async fn lookup(&self, prompt: &str, scope: CacheScope<'_>) -> Option<String> {
        let url = format!("{}/v1/cache/semantic/search", self.base_url);
        let payload = serde_json::json!({
            "org_id": scope.org_id,
            "user_id": scope.user_id,
            "model": scope.model,
            "prompt": prompt,
        });
        let bytes = serde_json::to_vec(&payload).ok()?;
        let request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(bytes);
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

    async fn store(&self, prompt: &str, scope: CacheScope<'_>, response: &str) {
        if response.is_empty() {
            return;
        }
        let url = format!("{}/v1/cache/semantic/store", self.base_url);
        let payload = serde_json::json!({
            "org_id": scope.org_id,
            "user_id": scope.user_id,
            "model": scope.model,
            "prompt": prompt,
            "response": response,
        });
        let Ok(bytes) = serde_json::to_vec(&payload) else {
            return;
        };
        let request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .body(bytes);
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

    /// Look up a cached response for `prompt`, scoped to org + model. Returns the
    /// cached text on a hit; any miss or error yields `None` so the caller falls
    /// through to inference.
    pub async fn lookup(&self, prompt: &str, scope: CacheScope<'_>) -> Option<String> {
        let body = SearchRequest {
            prompt,
            similarity_threshold: self.threshold,
            attributes: scope.attributes(),
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
    pub async fn store(&self, prompt: &str, scope: CacheScope<'_>, response: &str) {
        if response.is_empty() {
            return;
        }
        let body = StoreRequest {
            prompt,
            response,
            attributes: scope.attributes(),
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
    std::env::var(key).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
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

    /// The failure this policy exists to prevent: replaying answer TEXT for a
    /// turn that also emitted citations or a tool timeline, leaving the user
    /// with claims and no sources.
    fn test_scope() -> CacheScope<'static> {
        CacheScope {
            org_id: "org-1",
            user_id: "user-1",
            model: "model-x",
        }
    }

    #[test]
    fn a_turn_whose_answer_came_from_outside_the_prompt_is_never_cached() {
        for exclusion in [
            TurnCacheability {
                used_tools: true,
                ..TurnCacheability::default()
            },
            TurnCacheability {
                emitted_citations: true,
                ..TurnCacheability::default()
            },
            TurnCacheability {
                structured_output: true,
                ..TurnCacheability::default()
            },
            TurnCacheability {
                zdr: true,
                ..TurnCacheability::default()
            },
        ] {
            assert!(
                !exclusion.is_cacheable(),
                "{exclusion:?} must not be cacheable"
            );
        }
    }

    #[test]
    fn a_plain_conversational_turn_is_cacheable() {
        assert!(TurnCacheability::default().is_cacheable());
    }

    /// The leak this scoping exists to prevent: two colleagues in one org, one
    /// served an answer built from the other's memories.
    #[test]
    fn two_users_in_one_org_never_share_a_cache_key() {
        let scope = |user| CacheScope {
            org_id: "org-1",
            user_id: user,
            model: "m",
        };
        assert_ne!(
            DragonflyCache::key("hello", scope("user-a")),
            DragonflyCache::key("hello", scope("user-b")),
        );
        // A blank user must not collide with a named one either.
        assert_ne!(
            DragonflyCache::key("hello", scope("")),
            DragonflyCache::key("hello", scope("-")),
        );
        let attributes = scope("user-a").attributes();
        assert_eq!(
            attributes.get("user_id").map(String::as_str),
            Some("user-a")
        );
    }

    #[test]
    fn cache_key_is_scoped_stable_and_collision_guarded() {
        let scope = CacheScope {
            org_id: "org-1",
            user_id: "user-1",
            model: "m",
        };
        let key = DragonflyCache::key("hello", scope);
        assert_eq!(key, DragonflyCache::key("hello", scope), "stable");
        assert!(
            key.starts_with("mp:gw:cache:org-1:6#user-1:m:"),
            "namespaced by org + user + model"
        );
        assert_ne!(
            key,
            DragonflyCache::key(
                "hello",
                CacheScope {
                    org_id: "org-2",
                    ..scope
                }
            ),
            "org-scoped"
        );
        assert_ne!(
            key,
            DragonflyCache::key(
                "hello",
                CacheScope {
                    model: "m2",
                    ..scope
                }
            ),
            "model-scoped"
        );
        assert_ne!(
            key,
            DragonflyCache::key("HELLO", scope),
            "prompt is case-sensitive"
        );
        assert!(key.ends_with(":5"), "byte-length discriminator appended");
    }

    #[test]
    fn zdr_disables_cache_reads_and_writes_before_backend_selection() {
        assert!(!cache_io_allowed(true));
        assert!(cache_io_allowed(false));
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
        let hit = client.lookup("hello", test_scope()).await;
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
        assert!(client.lookup("hello", test_scope()).await.is_none());
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
        client.store("hello", test_scope(), "fresh").await;
        // MockServer verifies the expected POST on drop.
    }
}
