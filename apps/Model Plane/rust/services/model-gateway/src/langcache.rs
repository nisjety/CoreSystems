//! Redis `LangCache` semantic-cache client for the model-gateway.
//!
//! `LangCache` is a managed REST service that generates embeddings server-side,
//! so this client only ships prompt/response text plus scoping attributes
//! (`org_id` + `model`) — `org_id` keeps one tenant from reading another's
//! cached responses. The client is process-global and env-configured: when
//! `LANGCACHE_URL` / `LANGCACHE_CACHE_ID` / `LANGCACHE_API_KEY` are unset the
//! gateway runs without a cache (every `Invoke` hits inference-core), matching
//! the dev-friendly "disabled when unconfigured" pattern used elsewhere.
//!
//! The cache is strictly best-effort: any transport/parse error degrades to a
//! miss so inference still runs.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const DEFAULT_THRESHOLD: f64 = 0.9;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

static GLOBAL: OnceLock<Option<LangCacheClient>> = OnceLock::new();

/// Returns the process-global client, lazily initialized from the environment.
/// `None` means no cache is configured — callers proceed straight to inference.
pub fn global() -> Option<&'static LangCacheClient> {
    GLOBAL.get_or_init(LangCacheClient::from_env).as_ref()
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
