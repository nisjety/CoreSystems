//! Real web tools for the agentic loop — `web_search` + `web_fetch`.
//!
//! These replace the deterministic echo fallback in `tool_bridge` for the two
//! most useful read-only research tools, backed by the Quarry-v2 edge (the same
//! service the browser agent drives). Reuses `QUARRY_EDGE_URL` /
//! short-lived Auth Core service-principal tokens. Quarry's DTOs live in a
//! separate cargo workspace, so —
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

use mp_contracts::model_plane::v1 as pb;

use crate::quarry_auth::TokenSource;

const MAX_SEARCH_RESULTS: usize = 8;
// Second-stage snippet cap. The SERP providers already hand back ~150-char
// snippets, so the previous 300 was near-invisible in the common case and
// actively destructive on the long ones — an audit traced thin-source answers
// back to this truncation compounding the provider's own. `model-gateway`, the
// other client of the same edge, applies no snippet cap at all; this bound
// exists only to stop one pathological result from crowding the step's context
// window, not to summarize. Keep it well clear of real snippet lengths.
const MAX_SNIPPET_CHARS: usize = 900;
const MAX_FETCH_CHARS: usize = 8000;
const SEARCH_SCOPES: &[&str] = &["search:read"];
const EXTRACT_SCOPES: &[&str] = &["extract:read"];
/// E3.7.10: the scopes of Quarry's site map and its durable crawl/batch jobs. The
/// service principal's grant of these is a Control Plane registry row (MP-ASKS).
const MAP_SCOPES: &[&str] = &["map:read"];
const CRAWL_SCOPES: &[&str] = &["crawl:write"];
/// Ceiling on the public `GET /ready` probe — short on purpose so a hung edge
/// cannot stall the 60 s health heartbeat for the other attestations.
const READY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// HTTP client for Quarry edge search/extract. Cheap to clone.
#[derive(Clone)]
pub struct WebToolsClient {
    base_url: String,
    auth: TokenSource,
    http: reqwest::Client,
}

impl WebToolsClient {
    /// Build from the environment. Returns `None` when `QUARRY_EDGE_URL` /
    /// `QUARRY_EDGE_ADDR` is unset, so the caller can surface a clear
    /// "not configured" error instead of echoing.
    pub fn from_env() -> Result<Option<Self>, String> {
        let base_url = std::env::var("QUARRY_EDGE_URL")
            .or_else(|_| std::env::var("QUARRY_EDGE_ADDR"))
            .ok()
            .filter(|s| !s.trim().is_empty());
        let Some(base_url) = base_url else {
            return Ok(None);
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| format!("failed to build Quarry HTTP client: {error}"))?;
        let auth = TokenSource::from_env()
            .map_err(|error| format!("quarry authentication is not configured: {error}"))?;
        Ok(Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth,
            http,
        }))
    }

    async fn post(
        &self,
        path: &str,
        body: &Value,
        org_id: &str,
        scopes: &[&str],
    ) -> Result<Value, String> {
        let mut retried_unauthorized = false;
        let resp = loop {
            let bearer = self
                .auth
                .token(org_id, scopes)
                .await
                .map_err(|error| format!("quarry authentication failed: {error}"))?;
            let resp = self
                .http
                .post(format!("{}{path}", self.base_url))
                .bearer_auth(&bearer)
                .header("x-quarry-org", org_id)
                .json(body)
                .send()
                .await
                .map_err(|e| format!("quarry {path} request failed: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.auth
                    .invalidate_if_matches(org_id, scopes, &bearer)
                    .await
                    .map_err(|error| format!("quarry authentication failed: {error}"))?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
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
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        org_id: &str,
        zdr: bool,
    ) -> Result<String, String> {
        let body = search_body(query, limit, zdr);
        let value = self
            .post("/v1/search", &body, org_id, SEARCH_SCOPES)
            .await?;
        // `results` is top-level on purpose. `quarry-edge`'s handler returns
        // `(StatusCode::OK, Json(SearchResponse))` with no wrapping middleware,
        // so this IS the edge's contract. The `data: {...}` envelope some
        // callers expect is added by the BFF in front of the chat path, not by
        // the edge — do not "harmonise" this reader toward `data.results`.
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(format_search_results(query, &value, &results))
    }

    /// `web_fetch` → POST `/v1/extract` (no schema ⇒ cleaned markdown per URL).
    pub async fn fetch(&self, url: &str, org_id: &str, zdr: bool) -> Result<String, String> {
        let body = extract_body(url, zdr);
        let value = self
            .post("/v1/extract", &body, org_id, EXTRACT_SCOPES)
            .await?;
        extract_markdown(url, &value)
    }

    /// `knowledge.map` -> POST `/v1/map`: the URLs of a site (sitemaps and robots), optionally
    /// ranked by `search`. Nothing is fetched into the corpus and nothing is stored.
    pub async fn map_site(
        &self,
        url: &str,
        search: Option<&str>,
        limit: u32,
        include_subdomains: bool,
        org_id: &str,
    ) -> Result<String, String> {
        let body = map_body(url, search, limit, include_subdomains);
        let value = self.post("/v1/map", &body, org_id, MAP_SCOPES).await?;
        Ok(format_map_result(url, &value))
    }

    /// `knowledge.crawl_site` -> POST `/v1/crawl`: starts a background crawl job. The answer is
    /// the job's handoff (id and the bounds Quarry will actually run with), not the pages.
    pub async fn start_crawl(
        &self,
        url: &str,
        max_pages: u32,
        max_depth: Option<u32>,
        ingest: bool,
        org_id: &str,
    ) -> Result<String, String> {
        let body = crawl_body(url, max_pages, max_depth, ingest);
        let value = self.post("/v1/crawl", &body, org_id, CRAWL_SCOPES).await?;
        Ok(format_handoff("crawl", &value))
    }

    /// `knowledge.batch` -> POST `/v1/batch`: starts a background scrape job over a fixed list.
    pub async fn start_batch(
        &self,
        urls: &[String],
        ingest: bool,
        org_id: &str,
    ) -> Result<String, String> {
        let body = batch_body(urls, ingest);
        let value = self.post("/v1/batch", &body, org_id, CRAWL_SCOPES).await?;
        Ok(format_handoff("batch", &value))
    }

    /// Health probe behind the `cap.tool.http` attestation: `Ok(())` only when
    /// `web_search` and `web_fetch` could actually authenticate AND the edge is
    /// serving.
    ///
    /// Two things are checked, and both are required:
    ///
    /// 1. quarry-edge's public `GET /ready` answers `200` with `status` `ready`.
    /// 2. The exact credentials dispatch uses can be minted for BOTH scope sets it
    ///    asks for (`SEARCH_SCOPES` for `web_search`, `EXTRACT_SCOPES` for
    ///    `web_fetch`), through the same base URL and token source.
    ///
    /// `/ready` alone would be dishonest: it reports only the edge's durable flag,
    /// and the edge always carries a local search index, so `/v1/search` never
    /// even fails while Auth Core can still be refusing the credential. Today
    /// Auth Core refuses both scopes for this principal, so this returns `Err` and
    /// the capability stays unavailable until the Control Plane grants them.
    ///
    /// It NEVER calls `/v1/search` or `/v1/extract`: the search route meters a
    /// billable usage unit per query, so probing through it every heartbeat would
    /// bill an org once a minute. The mint is cached by the token provider, so the
    /// steady-state cost is two mints per token lifetime, not per probe.
    ///
    /// Nothing here logs or returns a token, a mint response or the service
    /// credential; the error strings carry statuses only.
    pub(crate) async fn probe_ready(&self, org_id: &str) -> Result<(), String> {
        self.probe_ready_with_timeout(org_id, READY_PROBE_TIMEOUT)
            .await
    }

    async fn probe_ready_with_timeout(
        &self,
        org_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        // A dedicated client rather than `self.http`: that one follows redirects
        // and carries a 60 s ceiling, and a readiness answer from a redirect
        // target is not the edge's own answer.
        let probe_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(|error| format!("failed to build Quarry readiness client: {error}"))?;
        let response = probe_http
            .get(format!("{}/ready", self.base_url))
            .send()
            .await
            .map_err(|error| format!("quarry /ready request failed: {error}"))?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            return Err(format!("quarry /ready returned {status}"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|error| format!("quarry /ready decode failed: {error}"))?;
        if body.get("status").and_then(Value::as_str) != Some("ready") {
            return Err("quarry /ready did not report status ready".to_owned());
        }
        // The credentials, in dispatch's own order and through its own source.
        // The minted values are dropped on the spot: the point is that Auth Core
        // grants them, not to use them.
        for scopes in [SEARCH_SCOPES, EXTRACT_SCOPES] {
            self.auth
                .token(org_id, scopes)
                .await
                .map_err(|error| format!("quarry authentication failed: {error}"))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(base_url: &str, auth: TokenSource) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// E3.7.10: the agent's bulk-acquisition tools (`knowledge.crawl_site`, `knowledge.map`,
// `knowledge.batch`), governed as `cap.web.crawl|map|batch`.
// ---------------------------------------------------------------------------------------------

/// The deployment switch for the three tools. Default OFF: only `1`, `true` or `on` enables it.
pub const CRAWL_TOOLS_ENV: &str = "EXECUTION_CORE_WEB_CRAWL_TOOLS";
/// Tighten-only page-count approval threshold: a value above [`DEFAULT_APPROVAL_PAGES`], zero or
/// a non-number leaves the default; a smaller one lowers it.
pub const CRAWL_APPROVAL_PAGES_ENV: &str = "EXECUTION_CORE_WEB_CRAWL_APPROVAL_PAGES";
/// A crawl or a batch over this many pages needs a person's approval (decision 2).
pub const DEFAULT_APPROVAL_PAGES: u32 = 50;
/// The most pages execution-core ever asks Quarry for, whatever the model writes. Quarry's own
/// limits resolver still applies on top.
pub const MAX_CRAWL_PAGES: u32 = 1000;
/// The most URLs one batch call may carry.
pub const MAX_BATCH_URLS: usize = 100;
const MAX_MAP_LINKS: usize = 100;
const MAX_MAP_LIMIT: u32 = 500;
pub const DEFAULT_MAP_LIMIT: u32 = 100;

pub const CRAWL_SITE_TOOL: &str = "knowledge.crawl_site";
pub const MAP_TOOL: &str = "knowledge.map";
pub const BATCH_TOOL: &str = "knowledge.batch";

/// Whether the deployment has switched the three tools on.
#[must_use]
pub fn crawl_tools_enabled() -> bool {
    crawl_tools_enabled_from(std::env::var(CRAWL_TOOLS_ENV).ok().as_deref())
}

#[must_use]
pub fn crawl_tools_enabled_from(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on")
    )
}

/// The effective approval threshold: [`DEFAULT_APPROVAL_PAGES`], or a smaller configured value.
/// Configuration can only tighten it.
#[must_use]
pub fn crawl_approval_pages() -> u32 {
    approval_pages_from(std::env::var(CRAWL_APPROVAL_PAGES_ENV).ok().as_deref())
}

#[must_use]
pub fn approval_pages_from(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|pages| *pages >= 1)
        .map_or(DEFAULT_APPROVAL_PAGES, |pages| pages.min(DEFAULT_APPROVAL_PAGES))
}

/// Whether `name` is one of the three tools.
#[must_use]
pub fn is_crawl_tool(name: &str) -> bool {
    matches!(name, CRAWL_SITE_TOOL | MAP_TOOL | BATCH_TOOL)
}

/// Whether `name` starts a durable Quarry job (a crawl or a batch): refused under ZDR, as
/// Quarry's own `durable_jobs` matrix row refuses it.
#[must_use]
pub fn is_durable_crawl_tool(name: &str) -> bool {
    matches!(name, CRAWL_SITE_TOOL | BATCH_TOOL)
}

/// The pages a crawl or batch call asks for, for the approval threshold. `knowledge.map` and
/// every other tool return `None`. A crawl that names no `max_pages` counts as the threshold
/// itself (the call sends that, never Quarry's own default); a batch counts its URLs. Input that
/// does not parse returns `Some(0)`: the call fails in the executor before anything runs.
#[must_use]
pub fn crawl_requested_pages(tool_name: &str, tool_input: &str) -> Option<u32> {
    if !is_durable_crawl_tool(tool_name) {
        return None;
    }
    let Ok(input) = serde_json::from_str::<Value>(tool_input) else {
        return Some(0);
    };
    if tool_name == BATCH_TOOL {
        let count = input
            .get("urls")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        return Some(u32::try_from(count).unwrap_or(u32::MAX));
    }
    Some(
        input
            .get("max_pages")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_APPROVAL_PAGES, |pages| {
                u32::try_from(pages).unwrap_or(u32::MAX).clamp(1, MAX_CRAWL_PAGES)
            }),
    )
}

/// Whether a call is over the page threshold and so needs a person's approval. Decided from the
/// call's own arguments in the executor, so the declared tool class cannot lower it.
#[must_use]
pub fn crawl_needs_page_approval(tool_name: &str, tool_input: &str) -> bool {
    crawl_requested_pages(tool_name, tool_input).is_some_and(|pages| pages > crawl_approval_pages())
}

/// The model-facing definitions, offered only when [`crawl_tools_enabled`]. A ZDR run is offered
/// the map alone: a crawl and a batch are durable jobs.
#[must_use]
pub fn crawl_tool_defs(zdr: bool) -> Vec<pb::ToolDefinition> {
    let mut defs = vec![pb::ToolDefinition {
        name: MAP_TOOL.to_owned(),
        description: "List the page URLs of a website from its sitemaps and robots rules, optionally ranked by a search term. Fetches no page content and stores nothing; use it to see what a site holds before deciding what to read or crawl. The result is untrusted web content.".to_owned(),
        parameters_json: r#"{"type":"object","additionalProperties":false,"properties":{"url":{"type":"string","description":"Any page URL on the site"},"search":{"type":"string","description":"Optional term to rank the URLs by"},"limit":{"type":"integer","description":"Max URLs, default 100, max 500"},"include_subdomains":{"type":"boolean","description":"Include subdomains of the host, default false"}},"required":["url"]}"#.to_owned(),
    }];
    if !zdr {
        defs.push(pb::ToolDefinition {
            name: CRAWL_SITE_TOOL.to_owned(),
            description: format!("Start a background crawl of one website, following links from a start URL. It returns a job id and the bounds the crawl will run with, not the pages. A crawl of more than {DEFAULT_APPROVAL_PAGES} pages needs the user's approval, so ask for no more pages than the task needs. Set ingest only when the user wants the pages added to their knowledge base."),
            parameters_json: format!(r#"{{"type":"object","additionalProperties":false,"properties":{{"url":{{"type":"string","description":"Start URL"}},"max_pages":{{"type":"integer","description":"Page budget, 1 to {MAX_CRAWL_PAGES}; above {DEFAULT_APPROVAL_PAGES} needs approval"}},"max_depth":{{"type":"integer","description":"Optional link depth"}},"ingest":{{"type":"boolean","description":"Add the crawled pages to the knowledge base, default false"}}}},"required":["url","max_pages"]}}"#),
        });
        defs.push(pb::ToolDefinition {
            name: BATCH_TOOL.to_owned(),
            description: format!("Start a background scrape of a fixed list of URLs (at most {MAX_BATCH_URLS}). It returns a job id, not the pages. A batch of more than {DEFAULT_APPROVAL_PAGES} URLs needs the user's approval. Set ingest only when the user wants the pages added to their knowledge base."),
            parameters_json: format!(r#"{{"type":"object","additionalProperties":false,"properties":{{"urls":{{"type":"array","items":{{"type":"string"}},"maxItems":{MAX_BATCH_URLS},"description":"The URLs to scrape"}},"ingest":{{"type":"boolean","description":"Add the pages to the knowledge base, default false"}}}},"required":["urls"]}}"#),
        });
    }
    defs
}

/// Body for `POST /v1/map` (pure).
fn map_body(url: &str, search: Option<&str>, limit: u32, include_subdomains: bool) -> Value {
    let mut body = json!({
        "url": url,
        "limit": limit.clamp(1, MAX_MAP_LIMIT),
        "include_subdomains": include_subdomains,
    });
    if let Some(term) = search.map(str::trim).filter(|term| !term.is_empty()) {
        body["search"] = json!(term);
    }
    body
}

/// Body for `POST /v1/crawl` (pure). `max_pages` is always sent, clamped, so Quarry's own
/// default never decides how many pages an approved-or-not call fetches. No `zdr` key: a ZDR
/// run never reaches this (dispatch refuses it), and Quarry reads the caller's claim anyway.
fn crawl_body(url: &str, max_pages: u32, max_depth: Option<u32>, ingest: bool) -> Value {
    let mut body = json!({
        "url": url,
        "max_pages": max_pages.clamp(1, MAX_CRAWL_PAGES),
        "ingest": ingest,
    });
    if let Some(depth) = max_depth.filter(|depth| *depth >= 1) {
        body["max_depth"] = json!(depth);
    }
    body
}

/// Body for `POST /v1/batch` (pure).
fn batch_body(urls: &[String], ingest: bool) -> Value {
    json!({ "urls": urls, "ingest": ingest })
}

/// The `data` object of Quarry's `Envelope`, or the value itself when it is not wrapped.
fn envelope_data(value: &Value) -> &Value {
    value.get("data").filter(|data| !data.is_null()).unwrap_or(value)
}

/// Format a `/v1/map` answer into agent-readable text (pure).
fn format_map_result(url: &str, value: &Value) -> String {
    let data = envelope_data(value);
    let links = data
        .get("links")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if links.is_empty() {
        return format!("No URLs found for {url}.");
    }
    let total = data
        .get("count")
        .and_then(Value::as_u64)
        .map_or(links.len(), |count| usize::try_from(count).unwrap_or(links.len()));
    let mut out = format!("{total} URLs found for {url} (showing {}):\n", links.len().min(MAX_MAP_LINKS));
    for link in links.iter().take(MAX_MAP_LINKS) {
        let href = link.get("url").and_then(Value::as_str).unwrap_or("");
        match link.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            Some(title) => {
                let _ = writeln!(out, "- {href} ({})", truncate_chars(title, 120));
            }
            None => {
                let _ = writeln!(out, "- {href}");
            }
        }
    }
    if data
        .get("coverage")
        .and_then(|coverage| coverage.get("truncated"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        out.push_str("(the site has more URLs than were returned)\n");
    }
    out
}

/// Format a crawl or batch handoff into agent-readable text (pure): the job id and the bounds.
fn format_handoff(kind: &str, value: &Value) -> String {
    let data = envelope_data(value);
    let job_id = data.get("job_id").and_then(Value::as_str).unwrap_or("");
    if job_id.is_empty() {
        return format!("The {kind} job was not accepted: no job id came back.");
    }
    let mut out = format!("Started {kind} job {job_id}; it runs in the background.");
    if let Some(limits) = data.get("limits").filter(|limits| !limits.is_null()) {
        let _ = write!(out, " Bounds: {limits}.");
    }
    if let Some(rejected) = data.get("rejected").and_then(Value::as_array) {
        if !rejected.is_empty() {
            let _ = write!(out, " {} URL(s) were refused before the job started.", rejected.len());
        }
    }
    out
}

/// Body for `POST /v1/search` (pure; keeps the wire shape assertable in tests).
///
/// `zdr` is not optional decoration: the edge's `SearchRequest.zdr` is what
/// makes it bypass its response cache and skip the durable content-bearing
/// events. Omitting the key defaults it to `false` server-side, which is why
/// agentic runs on a ZDR tenant were silently having fetched content cached
/// and persisted until this was threaded through.
///
/// `allow_paid_providers` is deliberately absent. Agentic runs are free-chain
/// only — a model-authored loop can issue searches without a human in the
/// loop, so metered SERP providers stay off this path by construction rather
/// than by budget. This omission is a decision, not an oversight; the chat
/// path is where paid providers get opted into.
fn search_body(query: &str, limit: u32, zdr: bool) -> Value {
    json!({ "query": query, "limit": limit, "zdr": zdr })
}

/// Body for `POST /v1/extract` (pure; see `search_body` on the `zdr` key).
fn extract_body(url: &str, zdr: bool) -> Value {
    json!({ "urls": [url], "zdr": zdr })
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
        let title = r
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(untitled)");
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

// `pub(crate)` so `health_attest`'s tests can reuse the Quarry stubs below.
#[cfg(test)]
pub(crate) mod tests {
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
        let value =
            json!({ "results": [{"url": "https://x", "status": "blocked", "markdown": ""}] });
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
    fn search_body_carries_zdr() {
        let on = search_body("rust", 8, true);
        assert_eq!(on["zdr"], json!(true));
        assert_eq!(on["query"], json!("rust"));
        assert_eq!(on["limit"], json!(8));
        // The flag must be sent explicitly in both directions: the edge
        // defaults a missing key to false, so an absent key is indistinguishable
        // from "retention allowed".
        assert_eq!(search_body("rust", 8, false)["zdr"], json!(false));
        // Free-chain-only by construction — see `search_body`'s doc.
        assert!(on.get("allow_paid_providers").is_none());
    }

    #[test]
    fn extract_body_carries_zdr() {
        let on = extract_body("https://example.com", true);
        assert_eq!(on["zdr"], json!(true));
        assert_eq!(on["urls"], json!(["https://example.com"]));
        assert_eq!(
            extract_body("https://example.com", false)["zdr"],
            json!(false)
        );
    }

    #[test]
    fn long_snippet_truncates_at_current_bound() {
        let long = "a".repeat(MAX_SNIPPET_CHARS + 50);
        let env = json!({
            "query": "q", "provider": "hybrid", "count": 1,
            "results": [{"title": "T", "url": "https://x", "snippet": long}]
        });
        let results = env.get("results").unwrap().as_array().unwrap().clone();
        let out = format_search_results("q", &env, &results);
        assert!(out.contains("truncated 50 chars"));

        // A snippet comfortably longer than the old 300-char cap must now
        // survive intact — that regression is what starved answers of source
        // text, so pin it rather than just pinning the cap constant.
        let survivor = "b".repeat(600);
        let env = json!({
            "query": "q", "provider": "hybrid", "count": 1,
            "results": [{"title": "T", "url": "https://x", "snippet": survivor}]
        });
        let results = env.get("results").unwrap().as_array().unwrap().clone();
        let out = format_search_results("q", &env, &results);
        assert!(out.contains(&survivor));
        assert!(!out.contains("truncated"));
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

    // ---- `probe_ready`: the cap.tool.http health probe -------------------------
    //
    // A `/ready`-only probe would be dishonest: quarry-edge's `/ready` reports
    // only its durable flag, and local Tantivy search means `/v1/search` never
    // even fails, so the edge looks healthy while Auth Core refuses the very
    // credentials dispatch needs. These pin that the probe asks for those
    // credentials too, and that it can never touch a metered route.

    use crate::quarry_auth::TokenProvider;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) const TEST_SERVICE_KEY: &str = "execution-core-test-credential";

    pub(crate) fn quarry_client(edge_url: &str, auth_url: &str) -> WebToolsClient {
        WebToolsClient::new_for_test(
            edge_url,
            TokenSource::ServicePrincipal(TokenProvider::new_for_test(
                auth_url,
                "execution-core",
                TEST_SERVICE_KEY,
            )),
        )
    }

    /// The mint request dispatch sends for ONE scope set — exact body, exact
    /// service headers — so a probe that asked for anything else would not match.
    pub(crate) fn mint_mock(org_id: &str, scope: &str, response: ResponseTemplate) -> Mock {
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", TEST_SERVICE_KEY))
            .and(body_json(json!({
                "orgId": org_id,
                "scopes": [scope],
                "reason": "execution-core Quarry access",
            })))
            .respond_with(response)
    }

    pub(crate) fn minted(token: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "token": token,
            "expiresInSeconds": 300,
            "audience": "quarry"
        }))
    }

    pub(crate) fn refused() -> ResponseTemplate {
        ResponseTemplate::new(403)
            .set_body_json(json!({"message": "Service principal is not authorized"}))
    }

    /// A quarry-edge stub whose METERED routes fail the test if they are ever
    /// called: `/v1/search` bills a usage unit per query, so a heartbeat probe
    /// that reached it would bill an org every minute.
    pub(crate) async fn edge_with_ready(ready: ResponseTemplate) -> MockServer {
        let edge = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ready"))
            .respond_with(ready)
            .mount(&edge)
            .await;
        for metered in ["/v1/search", "/v1/extract"] {
            Mock::given(method("POST"))
                .and(path(metered))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&edge)
                .await;
        }
        edge
    }

    pub(crate) fn ready_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "ready", "durable": true}))
    }

    pub(crate) async fn mounted_auth(
        search: ResponseTemplate,
        extract: ResponseTemplate,
        org_id: &str,
    ) -> MockServer {
        let auth = MockServer::start().await;
        mint_mock(org_id, "search:read", search).mount(&auth).await;
        mint_mock(org_id, "extract:read", extract)
            .mount(&auth)
            .await;
        auth
    }

    #[tokio::test]
    async fn probe_is_healthy_when_the_edge_is_ready_and_both_scopes_mint() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = MockServer::start().await;
        // Each scope set is minted exactly once, with exactly that scope: the
        // credentials dispatch uses, not a broader or different one.
        mint_mock("global", "search:read", minted("search-token"))
            .expect(1)
            .mount(&auth)
            .await;
        mint_mock("global", "extract:read", minted("extract-token"))
            .expect(1)
            .mount(&auth)
            .await;

        quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect("a ready edge whose credentials mint is healthy");

        let requests = edge.received_requests().await.expect("recorded requests");
        assert_eq!(
            requests.len(),
            1,
            "the probe must make exactly one edge call"
        );
        assert_eq!(requests[0].method.as_str(), "GET");
        assert_eq!(requests[0].url.path(), "/ready");
        // `/ready` is public: a minted token must never be sent to it.
        assert!(
            requests[0].headers.get("authorization").is_none(),
            "the public readiness route must not receive a bearer"
        );
    }

    #[tokio::test]
    async fn probe_mints_for_the_org_it_is_given_not_a_hardcoded_one() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(minted("a"), minted("b"), "probe-org").await;
        quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("probe-org")
            .await
            .expect("both mints carry the probe org");
        // Had the probe minted for any other org the strict body matchers would
        // have answered 404 and the probe would have failed above.
        assert_eq!(
            auth.received_requests().await.expect("recorded").len(),
            2,
            "one mint per scope set"
        );
    }

    /// Today's live state: Auth Core's registry grants execution-core only
    /// `browser:execute` and `scrape:write` on the quarry audience, so it
    /// refuses BOTH scopes dispatch asks for. The capability must stay
    /// unavailable — attesting it would claim a working web tool that cannot run.
    #[tokio::test]
    async fn refused_mints_for_both_scopes_keep_the_capability_unavailable() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(refused(), refused(), "global").await;
        let error = quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("a refused credential is unhealthy");
        assert!(
            error.contains("403"),
            "the refusal status is reported: {error}"
        );
        assert!(
            !error.contains(TEST_SERVICE_KEY),
            "no credential in the error: {error}"
        );
    }

    #[tokio::test]
    async fn a_refused_search_mint_alone_is_unhealthy() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(refused(), minted("extract-token"), "global").await;
        let error = quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("web_search could not authenticate, so cap.tool.http is not healthy");
        assert!(error.contains("403"), "{error}");
    }

    #[tokio::test]
    async fn a_refused_extract_mint_alone_is_unhealthy() {
        // cap.tool.http covers web_fetch too: granting only search:read must not
        // make the capability look healthy for the tool that still cannot run.
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(minted("search-token"), refused(), "global").await;
        let error = quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("web_fetch could not authenticate, so cap.tool.http is not healthy");
        assert!(error.contains("403"), "{error}");
    }

    #[tokio::test]
    async fn an_edge_that_is_not_ready_is_unhealthy_and_no_credential_is_minted() {
        let edge = edge_with_ready(
            ResponseTemplate::new(503)
                .set_body_json(json!({"status": "not_ready", "durable": false, "reason": "no db"})),
        )
        .await;
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(minted("never"))
            .expect(0)
            .mount(&auth)
            .await;
        let error = quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("503 not_ready is unhealthy");
        assert!(error.contains("503"), "{error}");
    }

    #[tokio::test]
    async fn a_ready_answer_that_does_not_say_ready_is_unhealthy() {
        for body in [
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "degraded", "durable": true})),
            ResponseTemplate::new(200).set_body_json(json!({"durable": true})),
            ResponseTemplate::new(200).set_body_string("<html>captive portal</html>"),
        ] {
            let edge = edge_with_ready(body).await;
            let auth = mounted_auth(minted("a"), minted("b"), "global").await;
            quarry_client(&edge.uri(), &auth.uri())
                .probe_ready("global")
                .await
                .expect_err("only a 200 that reports status ready is healthy");
        }
    }

    #[tokio::test]
    async fn a_redirecting_ready_route_is_not_followed() {
        let sink = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ready_ok())
            .expect(0)
            .mount(&sink)
            .await;
        let edge = edge_with_ready(
            ResponseTemplate::new(307).insert_header("location", format!("{}/ready", sink.uri())),
        )
        .await;
        let auth = mounted_auth(minted("a"), minted("b"), "global").await;
        quarry_client(&edge.uri(), &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("a redirect is not a readiness answer");
    }

    #[tokio::test]
    async fn an_unreachable_edge_is_unhealthy() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(minted("never"))
            .expect(0)
            .mount(&auth)
            .await;
        quarry_client("http://127.0.0.1:1", &auth.uri())
            .probe_ready("global")
            .await
            .expect_err("nothing is listening");
    }

    #[tokio::test]
    async fn a_hung_edge_is_cut_off_at_the_probe_timeout() {
        let edge = edge_with_ready(ready_ok().set_delay(Duration::from_secs(5))).await;
        let auth = mounted_auth(minted("a"), minted("b"), "global").await;
        let started = std::time::Instant::now();
        quarry_client(&edge.uri(), &auth.uri())
            .probe_ready_with_timeout("global", Duration::from_millis(250))
            .await
            .expect_err("a hung /ready is unhealthy");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the probe must give up at its own timeout, not the stub's delay"
        );
    }
}

/// E3.7.10: the bulk-acquisition tools' pure rules.
#[cfg(test)]
mod crawl_tool_tests {
    use super::*;

    #[test]
    fn the_crawl_tools_are_off_unless_explicitly_switched_on() {
        assert_eq!(CRAWL_TOOLS_ENV, "EXECUTION_CORE_WEB_CRAWL_TOOLS");
        assert!(!crawl_tools_enabled_from(None));
        for off in ["", "0", "false", "off", "no", "2", "yes"] {
            assert!(!crawl_tools_enabled_from(Some(off)), "{off:?}");
        }
        for on in ["1", "true", "TRUE", "on", " on "] {
            assert!(crawl_tools_enabled_from(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn the_page_threshold_defaults_to_fifty_and_configuration_can_only_lower_it() {
        assert_eq!(DEFAULT_APPROVAL_PAGES, 50);
        assert_eq!(approval_pages_from(None), 50);
        for unchanged in ["", "abc", "0", "-3", "51", "5000"] {
            assert_eq!(approval_pages_from(Some(unchanged)), 50, "{unchanged:?}");
        }
        assert_eq!(approval_pages_from(Some("10")), 10);
        assert_eq!(approval_pages_from(Some(" 1 ")), 1);
        assert_eq!(approval_pages_from(Some("50")), 50);
    }

    #[test]
    fn only_a_crawl_or_a_batch_over_the_threshold_needs_approval() {
        let crawl = |pages: u32| format!(r#"{{"url":"https://example.com","max_pages":{pages}}}"#);
        assert!(!crawl_needs_page_approval(CRAWL_SITE_TOOL, &crawl(50)));
        assert!(crawl_needs_page_approval(CRAWL_SITE_TOOL, &crawl(51)));
        // A crawl naming no budget counts as the threshold, never more; zero is raised to one.
        assert!(!crawl_needs_page_approval(CRAWL_SITE_TOOL, r#"{"url":"https://example.com"}"#));
        assert_eq!(crawl_requested_pages(CRAWL_SITE_TOOL, &crawl(0)), Some(1));
        // The ceiling clamps a huge number rather than overflowing.
        assert_eq!(
            crawl_requested_pages(CRAWL_SITE_TOOL, &crawl(4_000_000_000)),
            Some(MAX_CRAWL_PAGES)
        );
        let urls = |count: usize| {
            let list: Vec<String> = (0..count).map(|i| format!("https://example.com/{i}")).collect();
            serde_json::json!({ "urls": list }).to_string()
        };
        assert!(!crawl_needs_page_approval(BATCH_TOOL, &urls(50)));
        assert!(crawl_needs_page_approval(BATCH_TOOL, &urls(51)));
        // The map, and any other tool, never asks; unparseable input asks nothing (it fails).
        assert_eq!(crawl_requested_pages(MAP_TOOL, &crawl(900)), None);
        assert_eq!(crawl_requested_pages("web_fetch", &crawl(900)), None);
        assert_eq!(crawl_requested_pages(CRAWL_SITE_TOOL, "not json"), Some(0));
        assert!(!crawl_needs_page_approval(CRAWL_SITE_TOOL, "not json"));
    }

    #[test]
    fn the_definitions_are_valid_and_a_zdr_run_is_offered_the_map_alone() {
        let all = crawl_tool_defs(false);
        let names: Vec<&str> = all.iter().map(|def| def.name.as_str()).collect();
        assert_eq!(names, [MAP_TOOL, CRAWL_SITE_TOOL, BATCH_TOOL]);
        for def in &all {
            let schema: Value = serde_json::from_str(&def.parameters_json)
                .unwrap_or_else(|error| panic!("{}: {error}", def.name));
            assert_eq!(schema["type"], "object", "{}", def.name);
            assert!(is_crawl_tool(&def.name));
        }
        let zdr: Vec<String> = crawl_tool_defs(true).into_iter().map(|def| def.name).collect();
        assert_eq!(zdr, [MAP_TOOL]);
        assert!(!is_durable_crawl_tool(MAP_TOOL));
        assert!(is_durable_crawl_tool(CRAWL_SITE_TOOL) && is_durable_crawl_tool(BATCH_TOOL));
    }

    #[test]
    fn the_wire_bodies_are_bounded() {
        let map = map_body("https://example.com", Some("  pricing "), 9_999, true);
        assert_eq!(map["limit"], MAX_MAP_LIMIT);
        assert_eq!(map["search"], "pricing");
        assert_eq!(map["include_subdomains"], true);
        assert!(map_body("https://example.com", Some("  "), 0, false).get("search").is_none());
        assert_eq!(map_body("https://example.com", None, 0, false)["limit"], 1);

        let crawl = crawl_body("https://example.com", 5_000, Some(0), false);
        assert_eq!(crawl["max_pages"], MAX_CRAWL_PAGES);
        assert_eq!(crawl["ingest"], false);
        assert!(crawl.get("max_depth").is_none(), "a zero depth is left to Quarry");
        assert_eq!(crawl_body("https://example.com", 3, Some(2), true)["max_depth"], 2);

        let batch = batch_body(&["https://a.example".to_owned()], true);
        assert_eq!(batch["urls"][0], "https://a.example");
        assert_eq!(batch["ingest"], true);
    }

    #[test]
    fn answers_are_formatted_from_the_envelope_or_the_bare_object() {
        let wrapped = serde_json::json!({"data": {"links": [
            {"url": "https://example.com/a", "title": "A"},
            {"url": "https://example.com/b"}
        ], "count": 2, "coverage": {"truncated": true}}});
        let text = format_map_result("https://example.com", &wrapped);
        assert!(text.contains("2 URLs found"), "{text}");
        assert!(text.contains("- https://example.com/a (A)"), "{text}");
        assert!(text.contains("- https://example.com/b\n"), "{text}");
        assert!(text.contains("more URLs"), "{text}");
        assert!(format_map_result("https://x.example", &serde_json::json!({"links": []}))
            .starts_with("No URLs found"));

        let ack = serde_json::json!({"data": {"job_id": "job_1", "limits": {"max_pages": 20},
            "rejected": ["ftp://x"]}});
        let text = format_handoff("crawl", &ack);
        assert!(text.contains("Started crawl job job_1"), "{text}");
        assert!(text.contains("max_pages"), "{text}");
        assert!(text.contains("1 URL(s) were refused"), "{text}");
        assert!(format_handoff("batch", &serde_json::json!({"data": {}})).contains("not accepted"));
    }
}
