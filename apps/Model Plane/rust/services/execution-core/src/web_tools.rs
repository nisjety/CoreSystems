//! Real web tools for the agentic loop — `web_search` + `web_fetch`.
//!
//! These replace the deterministic echo fallback in `tool_bridge` for the two
//! most useful read-only research tools, backed by the Quarry-v2 edge (the same
//! service the browser agent drives). Reuses `QUARRY_EDGE_URL` /
//! `QUARRY_EDGE_TOKEN`. Quarry's DTOs live in a separate cargo workspace, so —
//! like `model-gateway`'s Fetch client — responses are parsed defensively as
//! `serde_json::Value` rather than mirrored structs.
//!
//! Transport (HTTP against `quarry-edge`):
//!   POST /v1/search   → ranked web results (`{query, provider, results[], count}`)
//!   POST /v1/extract  → cleaned markdown per URL (`{results:[{url,status,markdown}]}`)
//!
//! Both are read-only (not in `permission::is_risky_tool`), so they run under
//! `ask` posture without an approval gate.

// `# Errors` prose for the obvious `Result<String, String>` helpers is noise;
// `doc_markdown` over-flags wire tokens. Low-signal pedantic lints.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{json, Value};

const MAX_SEARCH_RESULTS: usize = 8;
const MAX_SNIPPET_CHARS: usize = 300;
const MAX_FETCH_CHARS: usize = 8000;

/// HTTP client for Quarry edge search/extract. Cheap to clone.
#[derive(Clone)]
pub struct WebToolsClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl WebToolsClient {
    /// Build from the environment. Returns `None` when `QUARRY_EDGE_URL` /
    /// `QUARRY_EDGE_ADDR` is unset, so the caller can surface a clear
    /// "not configured" error instead of echoing.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("QUARRY_EDGE_URL")
            .or_else(|_| std::env::var("QUARRY_EDGE_ADDR"))
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let token = std::env::var("QUARRY_EDGE_TOKEN").unwrap_or_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            token,
            http,
        })
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        // Always send a bearer: the edge's AUTH_DEV_BYPASS accepts any token but
        // still requires the header; a real token is used when configured.
        let bearer = if self.token.is_empty() {
            "dev"
        } else {
            self.token.as_str()
        };
        let resp = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("quarry {path} request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("quarry {path} decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!("quarry {path} returned {status}: {value}"));
        }
        Ok(value)
    }

    /// `web_search` → POST `/v1/search`. Returns a compact ranked list (title,
    /// url, snippet). Empty result set is a successful, informative response.
    pub async fn search(&self, query: &str, limit: u32) -> Result<String, String> {
        let value = self
            .post("/v1/search", json!({ "query": query, "limit": limit }))
            .await?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(format_search_results(query, &value, &results))
    }

    /// `web_fetch` → POST `/v1/extract` (no schema ⇒ cleaned markdown per URL).
    pub async fn fetch(&self, url: &str) -> Result<String, String> {
        let value = self.post("/v1/extract", json!({ "urls": [url] })).await?;
        extract_markdown(url, &value)
    }
}

/// Format `/v1/search` results into agent-readable text (pure; testable
/// without a live edge).
fn format_search_results(query: &str, envelope: &Value, results: &[Value]) -> String {
    if results.is_empty() {
        let provider = envelope
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return format!("No web results for \"{query}\" (provider: {provider}).");
    }
    let mut out = format!("Web results for \"{query}\":\n");
    for (i, r) in results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let title = r.get("title").and_then(Value::as_str).unwrap_or("(untitled)");
        let url = r.get("url").and_then(Value::as_str).unwrap_or("");
        let snippet = r
            .get("snippet")
            .or_else(|| r.get("content"))
            .or_else(|| r.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let _ = write!(
            out,
            "{}. {title}\n   {url}\n   {}\n",
            i + 1,
            truncate_chars(snippet, MAX_SNIPPET_CHARS)
        );
    }
    out
}

/// Pull the cleaned markdown for the first source out of an `/v1/extract`
/// response (pure; testable without a live edge).
fn extract_markdown(url: &str, value: &Value) -> Result<String, String> {
    let first = value
        .get("results")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    let Some(result) = first else {
        return Err(format!("no extract result for {url}"));
    };
    let markdown = result.get("markdown").and_then(Value::as_str).unwrap_or("");
    if markdown.trim().is_empty() {
        let status = result.get("status").and_then(Value::as_str).unwrap_or("");
        return Err(format!(
            "fetched {url} (status={status}) but no extractable content"
        ));
    }
    Ok(truncate_chars(markdown, MAX_FETCH_CHARS))
}

/// Char-boundary-safe truncation (never panics on multibyte input).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max).collect();
    let dropped = s.chars().count() - max;
    format!("{head}…[truncated {dropped} chars]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_formats_ranked_results() {
        let env = json!({
            "query": "rust", "provider": "hybrid", "count": 2,
            "results": [
                {"title": "The Rust Lang", "url": "https://rust-lang.org", "snippet": "A language empowering everyone."},
                {"title": "Rust Book", "url": "https://doc.rust-lang.org/book", "content": "Learn Rust."}
            ]
        });
        let results = env.get("results").unwrap().as_array().unwrap().clone();
        let out = format_search_results("rust", &env, &results);
        assert!(out.contains("1. The Rust Lang"));
        assert!(out.contains("https://rust-lang.org"));
        assert!(out.contains("empowering everyone"));
        // `content` is used as the snippet fallback.
        assert!(out.contains("2. Rust Book"));
        assert!(out.contains("Learn Rust."));
    }

    #[test]
    fn search_empty_is_informative_not_error() {
        let env = json!({ "query": "zzz", "provider": "hybrid", "results": [], "count": 0 });
        let out = format_search_results("zzz", &env, &[]);
        assert!(out.contains("No web results"));
        assert!(out.contains("provider: hybrid"));
    }

    #[test]
    fn extract_returns_first_source_markdown() {
        let value = json!({
            "results": [{"url": "https://example.com/", "status": "fetched", "markdown": "Example Domain\n====\n\nbody"}],
            "count": 1, "requested": 1
        });
        let md = extract_markdown("https://example.com", &value).expect("has markdown");
        assert!(md.contains("Example Domain"));
        assert!(md.contains("body"));
    }

    #[test]
    fn extract_empty_markdown_is_error() {
        let value = json!({ "results": [{"url": "https://x", "status": "blocked", "markdown": ""}] });
        let err = extract_markdown("https://x", &value).expect_err("empty markdown errors");
        assert!(err.contains("no extractable content"));
        assert!(err.contains("status=blocked"));
    }

    #[test]
    fn extract_no_results_is_error() {
        assert!(extract_markdown("https://x", &json!({ "results": [] })).is_err());
        assert!(extract_markdown("https://x", &json!({})).is_err());
    }

    #[test]
    fn truncate_is_char_safe() {
        // multibyte input must not panic and must report dropped count
        let s = "é".repeat(10);
        let out = truncate_chars(&s, 4);
        assert!(out.starts_with("éééé"));
        assert!(out.contains("truncated 6 chars"));
        assert_eq!(truncate_chars("short", 100), "short");
    }
}
