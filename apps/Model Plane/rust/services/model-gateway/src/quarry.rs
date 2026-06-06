//! Quarry-v2 edge client (Rust). Mirror of `go/pkg/quarry/client.go`.
//!
//! Used by the `Fetch` and `ExtractStructured` RPCs in
//! `crate::grpc::GatewayService`. Inline here rather than in a separate
//! workspace crate because no other Rust service in v1 currently needs
//! a Quarry client; promote to `rust/crates/mp-quarry` if that changes.
//!
//! See the proto contract in
//! `proto/model_plane/v1/gateway.proto::{FetchRequest, RenderHints}`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Typed Quarry error envelope (HTTP 4xx / 5xx with structured body).
#[derive(Debug, Error)]
pub enum QuarryError {
    /// Client wasn't wired with a base URL (caller should degrade).
    #[error("quarry: edge URL not configured")]
    Unavailable,

    /// Quarry returned a typed envelope error: {ok:false, error:{code,message}}.
    #[error("quarry: {code} (HTTP {status}): {message}")]
    Typed {
        code: String,
        message: String,
        status: u16,
    },

    /// Transport-level failure (DNS, refused, timeout, TLS).
    #[error("quarry: transport: {0}")]
    Transport(#[from] reqwest::Error),

    /// 2xx response was missing the `data` envelope wrapper.
    #[error("quarry: 2xx response missing 'data'")]
    EmptyEnvelope,

    /// Failed to parse the JSON envelope.
    #[error("quarry: decode envelope: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Browser-only render hints. Static / TLS-profile fetches ignore these.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RenderHints {
    #[serde(rename = "waitForSelector", skip_serializing_if = "Option::is_none")]
    pub wait_for_selector: Option<String>,
    #[serde(rename = "waitForTimeoutMs", skip_serializing_if = "Option::is_none")]
    pub wait_for_timeout_ms: Option<u32>,
}

impl RenderHints {
    /// True when the hints would actually change the fetch path. Used
    /// by `Client::scrape` to decide whether to include the `render`
    /// field at all (Quarry tolerates empty objects but we keep wire
    /// shape minimal).
    fn has_any(&self) -> bool {
        self.wait_for_selector.is_some()
    }
}

/// Projected `SearchResult`. Returned by [`Client::search`]; the full
/// Quarry envelope is kept under `raw` so callers can read additional
/// fields (rank features, provider-specific scores) without a new
/// projection round trip.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
    /// Provider that served this hit. Examples emitted by Quarry's
    /// `SmartSearchRouter`: `tantivy_local`, `tavily`, `bing`, `google`.
    pub source: String,
    /// Relevance score 0.0–1.0. Provider-specific normalisation —
    /// only meaningful within a single search response.
    pub score: f32,
    pub raw: Value,
}

/// Projected `ScrapeResult`. Full envelope is kept under `raw` for
/// callers that need branding, JSON-LD, links, etc.
#[derive(Debug, Clone)]
pub struct ScrapeResult {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub title: String,
    pub markdown: String,
    pub text: String,
    pub fingerprint: String,
    pub language: String,
    pub raw: Value,
}

/// Construction options for [`Client`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Base URL of the Quarry edge (e.g. `http://quarry-edge:8082`).
    /// Empty / unset → [`Client`] returns [`QuarryError::Unavailable`]
    /// on every call.
    pub base_url: String,
    /// Bearer token. Empty in dev (the edge's `AUTH_DEV_BYPASS` accepts
    /// any non-empty value in non-prod). Production MUST set this.
    pub token: String,
    /// End-to-end timeout per Scrape call. Default 30s — generous so
    /// JS-rendered pages have headroom.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            token: String::new(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// HTTP client for Quarry `/v1/scrape`. Cheap to clone (just an Arc
/// reference into reqwest's internal connection pool).
#[derive(Clone, Debug)]
pub struct Client {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl Client {
    /// Build a client. Returns the empty form (which reports
    /// `Available() == false`) when `cfg.base_url` is empty.
    #[must_use]
    pub fn new(cfg: Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .build()
            // Practically unreachable for the trivial config above;
            // fall back to a default client so construction is
            // infallible (callers don't have to thread a Result through
            // AppState initialisation).
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            token: cfg.token,
            http,
        }
    }

    /// True when the client was wired with a base URL.
    #[must_use]
    pub fn available(&self) -> bool {
        !self.base_url.is_empty()
    }

    /// POST `/v1/scrape` and project the response.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] if the edge is unavailable, the URL is empty,
    /// the upstream returns a non-2xx status, or the response cannot be decoded.
    pub async fn scrape(
        &self,
        url: &str,
        org_id: &str,
        render: Option<&RenderHints>,
        prefer_http3: bool,
    ) -> Result<ScrapeResult, QuarryError> {
        #[derive(Serialize)]
        struct Body<'a> {
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            render: Option<&'a RenderHints>,
            #[serde(skip_serializing_if = "is_false", rename = "prefer_http3")]
            prefer_http3: bool,
        }
        // serde requires fn(&T)->bool for skip_serializing_if
        #[allow(clippy::trivially_copy_pass_by_ref)]
        fn is_false(b: &bool) -> bool {
            !*b
        }

        if !self.available() {
            return Err(QuarryError::Unavailable);
        }
        if url.is_empty() {
            return Err(QuarryError::Typed {
                code: "BAD_REQUEST".to_string(),
                message: "url is required".to_string(),
                status: 400,
            });
        }

        let body = Body {
            url,
            render: render.filter(|r| r.has_any()),
            prefer_http3,
        };

        let endpoint = format!("{}/v1/scrape", self.base_url);
        let mut req = self.http.post(&endpoint).json(&body);
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }
        if !org_id.is_empty() {
            req = req.header("X-Quarry-Org", org_id);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let raw = resp.text().await?;

        if status >= 400 {
            // Best-effort decode of the typed envelope; tolerate
            // arbitrary bodies from upstream proxies / CDNs.
            #[derive(Deserialize)]
            struct EnvErr {
                error: Option<ErrPayload>,
            }
            #[derive(Deserialize)]
            struct ErrPayload {
                code: Option<String>,
                message: Option<String>,
            }
            let parsed: Option<EnvErr> = serde_json::from_str(&raw).ok();
            let (code, message) = parsed
                .and_then(|p| p.error)
                .map_or((None, None), |e| (e.code, e.message));
            return Err(QuarryError::Typed {
                code: code.unwrap_or_else(|| format!("HTTP_{status}")),
                message: message.unwrap_or_else(|| truncate(&raw, 200)),
                status,
            });
        }

        let env: serde_json::Map<String, Value> = serde_json::from_str(&raw)?;
        let data = env
            .get("data")
            .and_then(Value::as_object)
            .ok_or(QuarryError::EmptyEnvelope)?;

        Ok(project(url, &Value::Object(data.clone())))
    }

    /// Call `/v1/search` and return the projected results. The edge's
    /// `SmartSearchRouter` picks the provider (local Tantivy, Tavily,
    /// Bing, Google) based on the `intent` hint and operator config.
    ///
    /// Limit is capped at 50 to keep responses sane; callers needing
    /// more should paginate via the underlying provider.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] if the edge is unavailable, the query is empty,
    /// the upstream returns a non-2xx status, or the response cannot be decoded.
    pub async fn search(
        &self,
        query: &str,
        limit: i32,
        intent: &str,
        org_id: &str,
    ) -> Result<Vec<SearchResult>, QuarryError> {
        #[derive(Serialize)]
        struct Body<'a> {
            query: &'a str,
            limit: i32,
            #[serde(skip_serializing_if = "str::is_empty")]
            intent: &'a str,
        }

        if !self.available() {
            return Err(QuarryError::Unavailable);
        }
        if query.is_empty() {
            return Err(QuarryError::Typed {
                code: "BAD_REQUEST".to_string(),
                message: "query is required".to_string(),
                status: 400,
            });
        }
        // Server-side cap; an i32 limit comes in over the wire.
        let effective_limit = limit.clamp(1, 50);

        let body = Body {
            query,
            limit: effective_limit,
            intent,
        };

        let endpoint = format!("{}/v1/search", self.base_url);
        let mut req = self.http.post(&endpoint).json(&body);
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }
        if !org_id.is_empty() {
            req = req.header("X-Quarry-Org", org_id);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let raw = resp.text().await?;
        if status >= 400 {
            return Err(QuarryError::Typed {
                code: format!("HTTP_{status}"),
                message: truncate(&raw, 200),
                status,
            });
        }
        let payload: Value = serde_json::from_str(&raw)?;
        Ok(extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect())
    }
}

fn extract_search_results(payload: &Value) -> Vec<Value> {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| payload.get("results").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

// reason: provider scores are small; i64/f64→f32 loses no meaningful precision
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn project_search_result(v: &Value) -> SearchResult {
    let obj = v.as_object().cloned().unwrap_or_default();
    let source = string_field(&obj, "source");
    SearchResult {
        url: string_field(&obj, "url"),
        title: string_field(&obj, "title"),
        snippet: string_field(&obj, "snippet"),
        source: if source.is_empty() {
            string_field(&obj, "provider")
        } else {
            source
        },
        // Score may be float or int depending on provider; coerce both.
        score: obj
            .get("score")
            .and_then(|s| s.as_f64().or_else(|| s.as_i64().map(|n| n as f64)))
            .map_or(0.0, |f| f as f32),
        raw: Value::Object(obj),
    }
}

fn project(requested: &str, data: &Value) -> ScrapeResult {
    let obj = data.as_object().cloned().unwrap_or_default();
    let status = obj
        .get("status")
        .and_then(Value::as_u64)
        .map_or(0, |n| u16::try_from(n).unwrap_or(0));

    let content_type = string_field(&obj, "content_type");
    let fingerprint = string_field(&obj, "fingerprint");

    let final_url = obj
        .get("url")
        .and_then(Value::as_object)
        .and_then(|u| u.get("final"))
        .and_then(Value::as_str)
        .unwrap_or(requested)
        .to_string();

    let (markdown, text) = obj
        .get("formats")
        .and_then(Value::as_object)
        .map(|f| {
            let md = f
                .get("markdown")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let txt = f
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // Fall back so `.text` is always populated.
            let resolved_text = if txt.is_empty() { md.clone() } else { txt };
            (md, resolved_text)
        })
        .unwrap_or_default();

    let (title, language) = obj
        .get("metadata")
        .and_then(Value::as_object)
        .map(|m| {
            (
                m.get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                m.get("lang")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .unwrap_or_default();

    ScrapeResult {
        url: requested.to_string(),
        final_url,
        status,
        content_type,
        title,
        markdown,
        text,
        fingerprint,
        language,
        raw: Value::Object(obj),
    }
}

fn string_field(obj: &serde_json::Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        // Char-boundary-safe truncate
        let mut end = n;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        s[..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_base_url_is_unavailable() {
        let c = Client::new(Config::default());
        assert!(!c.available());
    }

    #[tokio::test]
    async fn unavailable_client_returns_unavailable_error() {
        let c = Client::new(Config::default());
        let err = c
            .scrape("https://example.com", "", None, false)
            .await
            .expect_err("must error");
        assert!(matches!(err, QuarryError::Unavailable));
    }

    #[test]
    fn render_hints_serialize_camelcase() {
        let h = RenderHints {
            wait_for_selector: Some("#ready".to_string()),
            wait_for_timeout_ms: Some(2500),
        };
        let s = serde_json::to_string(&h).unwrap();
        assert!(s.contains("\"waitForSelector\":\"#ready\""), "got: {s}");
        assert!(s.contains("\"waitForTimeoutMs\":2500"), "got: {s}");
    }

    #[test]
    fn render_hints_has_any_requires_selector() {
        assert!(!RenderHints::default().has_any());
        assert!(RenderHints {
            wait_for_selector: Some("#x".to_string()),
            wait_for_timeout_ms: None,
        }
        .has_any());
    }

    #[test]
    fn project_handles_missing_fields() {
        let data = serde_json::json!({"status": 200});
        let r = project("https://example.com", &data);
        assert_eq!(r.status, 200);
        assert_eq!(r.final_url, "https://example.com"); // falls back to requested
        assert!(r.markdown.is_empty());
        assert!(r.title.is_empty());
    }

    #[test]
    fn project_populates_text_from_markdown_when_missing() {
        let data = serde_json::json!({
            "status": 200,
            "formats": {"markdown": "# hello"}
        });
        let r = project("https://example.com", &data);
        assert_eq!(r.text, "# hello"); // text falls back to markdown
    }

    #[test]
    fn extract_search_results_accepts_bare_search_responses() {
        let payload = serde_json::json!({
            "query": "OpenAI",
            "provider": "smart_router",
            "results": [{
                "url": "https://openai.com/",
                "title": "OpenAI",
                "snippet": "Research and deployment.",
                "provider": "brave",
                "rank": 1
            }],
            "count": 1
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://openai.com/");
        assert_eq!(results[0].title, "OpenAI");
        assert_eq!(results[0].snippet, "Research and deployment.");
        assert_eq!(results[0].source, "brave");
    }

    #[test]
    fn extract_search_results_accepts_enveloped_search_responses() {
        let payload = serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://example.com/docs",
                    "title": "Docs",
                    "snippet": "Reference docs.",
                    "source": "searxng",
                    "score": 0.82
                }]
            }
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/docs");
        assert_eq!(results[0].source, "searxng");
        assert!((results[0].score - 0.82).abs() < f32::EPSILON);
    }
}
