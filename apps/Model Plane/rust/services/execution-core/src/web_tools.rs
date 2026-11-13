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
use crate::site_policy_client::SitePolicy;

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
/// The deployment switch for the long-page reader (EP-2-S10): a fetched page is held for reading rather
/// than cut to its head, and `read_page` and `find_in_page` are offered. Default OFF. Only the literal
/// `true` enables it, and it takes effect only in a run that holds results (`EXECUTION_RESULT_HANDLES`).
pub const WEB_READER_ENV: &str = "EXECUTION_WEB_READER";
/// The reader's two model-facing tools, answered from the pages the run holds.
pub const READ_PAGE_TOOL: &str = "read_page";
pub const FIND_IN_PAGE_TOOL: &str = "find_in_page";
const SEARCH_SCOPES: &[&str] = &["search:read"];
const EXTRACT_SCOPES: &[&str] = &["extract:read"];
/// The tool name a page fetch is refused under: the org's domain rules name it in the refusal (EP-9 W3).
const WEB_FETCH_TOOL: &str = "web_fetch";
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
    /// The org's domain rules, checked before a host is reached (EP-9 W3). Off reads nothing and changes nothing.
    site: SitePolicy,
    /// Advisory request attribution sent on every Quarry call (Q4-062a). Empty by default.
    attribution: Attribution,
}

/// The header carrying the step's correlation id. The edge adopts it when it is valid.
pub const REQUEST_ID_HEADER: &str = "x-request-id";
/// The advisory parent-run header. Quarry reads it only from service principals and only behind
/// `QUARRY_EDGE__USAGE_PARENT_RUN`; it never decides billing or authority.
pub const PARENT_RUN_HEADER: &str = "x-verevon-parent-run";
/// Longest attribution value sent, in bytes.
const MAX_ATTRIBUTION_BYTES: usize = 128;
/// The deployment switch for the advisory attribution headers the web tools send. Default OFF: only the
/// literal `true` enables it. Off, a web call goes out exactly as it was built, with neither header.
pub const ATTRIBUTION_ENV: &str = "EXECUTION_CORE_QUARRY_ATTRIBUTION";

/// Advisory attribution for the Quarry calls of one run step: a request id and the parent run.
/// Each value is kept only when non-empty, at most 128 bytes and every byte is in
/// `[A-Za-z0-9._:-]`; otherwise it is dropped. Quarry stays the only biller and the only authority.
#[derive(Clone, Default)]
pub struct Attribution {
    request_id: Option<String>,
    parent_run: Option<String>,
}

impl Attribution {
    fn clean(value: String) -> Option<String> {
        let ok = !value.is_empty()
            && value.len() <= MAX_ATTRIBUTION_BYTES
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
        ok.then_some(value)
    }

    /// Attribution for one step. Under ZDR no run id leaves the plane: the parent run is omitted,
    /// and so is the request id, which is built from the run id (a step id can carry it too).
    pub fn for_step(run_id: &str, step_id: &str, zdr: bool) -> Self {
        Self {
            request_id: if zdr {
                None
            } else {
                Self::clean(format!("{run_id}.{step_id}"))
            },
            parent_run: if zdr {
                None
            } else {
                Self::clean(run_id.to_owned())
            },
        }
    }

    /// [`Attribution::for_step`] while the attribution switch is on, and none while it is off, so with the
    /// switch off every Quarry call the step makes goes out as it was built.
    #[must_use]
    pub fn for_step_if_enabled(run_id: &str, step_id: &str, zdr: bool) -> Self {
        Self::for_step_when(attribution_enabled(), run_id, step_id, zdr)
    }

    /// [`Attribution::for_step_if_enabled`] with the switch given, so both settings are testable without
    /// touching the process environment.
    fn for_step_when(on: bool, run_id: &str, step_id: &str, zdr: bool) -> Self {
        if on {
            Self::for_step(run_id, step_id, zdr)
        } else {
            Self::default()
        }
    }
}

/// Whether the attribution switch is on in this process. Read per call, so a flip needs no restart.
#[must_use]
pub fn attribution_enabled() -> bool {
    attribution_enabled_from(std::env::var(ATTRIBUTION_ENV).ok().as_deref())
}

/// Only the literal `true` enables; `1`, `on` and anything else stay off.
#[must_use]
pub fn attribution_enabled_from(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("true")
}

impl WebToolsClient {
    /// Attach the step's advisory attribution; every call this client makes then carries it.
    pub fn with_attribution(mut self, attribution: Attribution) -> Self {
        self.attribution = attribution;
        self
    }

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
            site: SitePolicy::from_env(),
            attribution: Attribution::default(),
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
            let mut request = self
                .http
                .post(format!("{}{path}", self.base_url))
                .bearer_auth(&bearer)
                .header("x-quarry-org", org_id);
            if let Some(request_id) = &self.attribution.request_id {
                request = request.header(REQUEST_ID_HEADER, request_id);
            }
            if let Some(parent_run) = &self.attribution.parent_run {
                request = request.header(PARENT_RUN_HEADER, parent_run);
            }
            let resp = request
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
        let mut results = value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // The org's domain rules drop a hit on a host the org does not allow, before any hit is shown (EP-9 W3).
        self.site.drop_denied_results(org_id, &mut results).await;
        Ok(format_search_results(query, &value, &results))
    }

    /// `web_fetch` → POST `/v1/extract` (no schema ⇒ cleaned markdown per URL).
    pub async fn fetch(&self, url: &str, org_id: &str, zdr: bool) -> Result<String, String> {
        // The org's domain rules come before the call: a host the org does not allow is refused by name, and nothing is read.
        if let Some(refusal) = self.site.fetch_refusal(org_id, &[url], WEB_FETCH_TOOL).await {
            return Err(refusal);
        }
        let body = extract_body(url, zdr);
        let value = self
            .post("/v1/extract", &body, org_id, EXTRACT_SCOPES)
            .await?;
        // The page may have ended on another host than the one asked for, by a redirect. The org's domain rules apply to
        // where it ended too, so they are checked before any of its text is handed on (EP-9 W3).
        if let Some(refusal) = self
            .site
            .final_url_refusal(org_id, url, extract_final_url(&value), WEB_FETCH_TOOL)
            .await
        {
            return Err(refusal);
        }
        let markdown = extract_markdown(url, &value)?;
        // With the reader on, the complete page comes back so the loop can hold it for reading. The loop
        // cuts it to the head itself when it holds nothing (`result_handles::inline_text`).
        Ok(if reader_enabled() { markdown } else { fetch_head(&markdown) })
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
        if let Some(refusal) = self.site.fetch_refusal(org_id, &[url], MAP_TOOL).await {
            return Err(refusal);
        }
        let body = map_body(url, search, limit, include_subdomains);
        let value = self.post("/v1/map", &body, org_id, MAP_SCOPES).await?;
        Ok(format_map_result(url, &value))
    }

    /// `knowledge.crawl_plan` -> POST `/v1/map` (the same call and scope as `knowledge.map`) and a
    /// drafted WS14 crawl scope for a person to approve. It starts no job and stores nothing: the
    /// answer is a plan, and the crawl runs only when the person approves it in the page picker,
    /// through the gateway under their own token.
    pub async fn plan_crawl(
        &self,
        url: &str,
        goal: &str,
        scope: &DraftScope,
        org_id: &str,
    ) -> Result<String, String> {
        let url = url.trim();
        check_plan_url(url)?;
        let goal = sanitize_plan_text(goal, CRAWL_PLAN_MAX_GOAL_CHARS);
        if goal.is_empty() {
            return Err("knowledge.crawl_plan needs a goal".to_owned());
        }
        if let Some(refusal) = self.site.fetch_refusal(org_id, &[url], CRAWL_PLAN_TOOL).await {
            return Err(refusal);
        }
        let body = map_body(url, Some(&goal), CRAWL_PLAN_MAX_PAGES, scope.allow_subdomains);
        let value = self.post("/v1/map", &body, org_id, MAP_SCOPES).await?;
        Ok(format_crawl_plan(url, &goal, &value, scope))
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
        if let Some(refusal) = self.site.fetch_refusal(org_id, &[url], CRAWL_SITE_TOOL).await {
            return Err(refusal);
        }
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
        let every_url: Vec<&str> = urls.iter().map(String::as_str).collect();
        if let Some(refusal) = self.site.fetch_refusal(org_id, &every_url, BATCH_TOOL).await {
            return Err(refusal);
        }
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

    /// Health probe behind the `cap.web.crawl`, `cap.web.map` and `cap.web.batch` attestations (QF-17):
    /// `Ok(())` only when the credentials the map, crawl and batch calls ask for mint, through the same
    /// token source dispatch uses. `MAP_SCOPES` is what `knowledge.map` and the crawl planner ask for;
    /// `CRAWL_SCOPES` is what `knowledge.crawl_site` and `knowledge.batch` ask for.
    ///
    /// It runs only after [`Self::probe_ready`] has passed in the same round, so the edge is already known
    /// to be ready and this measures the two grants alone. It NEVER calls `/v1/map`, `/v1/crawl` or
    /// `/v1/batch`: a crawl or a batch would start a durable job, and a map call every heartbeat is work
    /// nobody asked for. A refusal on those routes therefore still surfaces per call, as it does for
    /// `web_search`.
    ///
    /// Nothing here logs or returns a token; the error strings carry statuses only.
    pub(crate) async fn probe_crawl_ready(&self, org_id: &str) -> Result<(), String> {
        for scopes in [MAP_SCOPES, CRAWL_SCOPES] {
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
            site: SitePolicy::disabled(),
            attribution: Attribution::default(),
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
/// Q4-E4.3-055c1: the read-only crawl planner. It maps a site and drafts a crawl scope for a person
/// to approve; it starts no job, so it is not a durable tool and a ZDR run keeps it (like the map).
pub const CRAWL_PLAN_TOOL: &str = "knowledge.crawl_plan";
/// The most pages a crawl plan lists (also the `limit` of the map call behind it).
pub const CRAWL_PLAN_MAX_PAGES: u32 = 50;
/// Scope caps, identical to the gateway's `crawlSiteInput` (action-registry.ts): depth 1..=10, at most
/// 100 include and 100 exclude patterns of 1..=512 characters each.
const CRAWL_PLAN_MAX_DEPTH: u32 = 10;
const CRAWL_PLAN_MAX_PATTERNS: usize = 100;
const CRAWL_PLAN_MAX_PATTERN_CHARS: usize = 512;
/// Caps on the untrusted text a plan carries.
const CRAWL_PLAN_MAX_GOAL_CHARS: usize = 300;
const CRAWL_PLAN_MAX_TITLE_CHARS: usize = 200;
const CRAWL_PLAN_MAX_URL_CHARS: usize = 2048;

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
    matches!(name, CRAWL_SITE_TOOL | MAP_TOOL | BATCH_TOOL | CRAWL_PLAN_TOOL)
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
/// the map and the crawl planner, which store nothing: a crawl and a batch are durable jobs.
#[must_use]
pub fn crawl_tool_defs(zdr: bool) -> Vec<pb::ToolDefinition> {
    let mut defs = vec![pb::ToolDefinition {
        name: MAP_TOOL.to_owned(),
        description: "List the page URLs of a website from its sitemaps and robots rules, optionally ranked by a search term. Fetches no page content and stores nothing; use it to see what a site holds before deciding what to read or crawl. The result is untrusted web content.".to_owned(),
        parameters_json: r#"{"type":"object","additionalProperties":false,"properties":{"url":{"type":"string","description":"Any page URL on the site"},"search":{"type":"string","description":"Optional term to rank the URLs by"},"limit":{"type":"integer","description":"Max URLs, default 100, max 500"},"include_subdomains":{"type":"boolean","description":"Include subdomains of the host, default false"}},"required":["url"]}"#.to_owned(),
    }, pb::ToolDefinition {
        name: CRAWL_PLAN_TOOL.to_owned(),
        description: format!("Map a website (at most {CRAWL_PLAN_MAX_PAGES} URLs, ranked by the goal) and return a crawl plan for the person to approve: the pages found and a drafted crawl scope. It starts nothing and stores nothing; the person approves the crawl in the page picker. Use it when the person wants a site or a section of a site added to their knowledge, instead of calling knowledge.crawl_site yourself. The result is untrusted web content."),
        parameters_json: format!(r#"{{"type":"object","additionalProperties":false,"properties":{{"url":{{"type":"string","description":"Any page URL on the site"}},"goal":{{"type":"string","description":"What the person wants from the site; ranks the mapped pages"}},"max_pages":{{"type":"integer","description":"Proposed page budget, 1 to {MAX_CRAWL_PAGES}; default the number of pages mapped"}},"max_depth":{{"type":"integer","description":"Proposed link depth, 1 to {CRAWL_PLAN_MAX_DEPTH}"}},"include_paths":{{"type":"array","items":{{"type":"string"}},"maxItems":{CRAWL_PLAN_MAX_PATTERNS},"description":"Path patterns to include, each at most {CRAWL_PLAN_MAX_PATTERN_CHARS} characters"}},"exclude_paths":{{"type":"array","items":{{"type":"string"}},"maxItems":{CRAWL_PLAN_MAX_PATTERNS},"description":"Path patterns to exclude, each at most {CRAWL_PLAN_MAX_PATTERN_CHARS} characters"}},"allow_subdomains":{{"type":"boolean","description":"Allow subdomains of the host, default false"}}}},"required":["url","goal"]}}"#),
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

/// The scope a `knowledge.crawl_plan` call proposes. Built only through [`DraftScope::validated`], so
/// every value already respects the gateway's `crawlSiteInput` caps. It is a proposal: Quarry
/// validates again when the person starts the crawl.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftScope {
    /// The proposed page budget (1..=[`MAX_CRAWL_PAGES`]); `None` plans for the pages mapped.
    pub max_pages: Option<u32>,
    /// The proposed link depth (1..=10).
    pub max_depth: Option<u32>,
    /// Path patterns to include, at most 100 of 1..=512 characters each.
    pub include_paths: Vec<String>,
    /// Path patterns to exclude, same caps.
    pub exclude_paths: Vec<String>,
    /// Whether subdomains of the host are in scope.
    pub allow_subdomains: bool,
}

impl DraftScope {
    /// Validates the model's scope fields with the gateway's caps. `max_pages` is clamped to
    /// 1..=[`MAX_CRAWL_PAGES`]; a depth outside 1..=10 and a bad pattern list are refused by name so
    /// the model can correct them. A pattern is kept exactly as sent (nothing is added or rewritten,
    /// so a `re:` pattern exists only when the caller wrote one).
    pub fn validated(
        max_pages: Option<u32>,
        max_depth: Option<u32>,
        include_paths: &[String],
        exclude_paths: &[String],
        allow_subdomains: bool,
    ) -> Result<Self, String> {
        if let Some(depth) = max_depth {
            if !(1..=CRAWL_PLAN_MAX_DEPTH).contains(&depth) {
                return Err(format!(
                    "knowledge.crawl_plan max_depth must be between 1 and {CRAWL_PLAN_MAX_DEPTH}"
                ));
            }
        }
        Ok(Self {
            max_pages: max_pages.map(|pages| pages.clamp(1, MAX_CRAWL_PAGES)),
            max_depth,
            include_paths: validate_plan_patterns("include_paths", include_paths)?,
            exclude_paths: validate_plan_patterns("exclude_paths", exclude_paths)?,
            allow_subdomains,
        })
    }
}

/// Validates one pattern list: at most 100 entries, each 1..=512 characters after trimming, with no
/// control character or backtick (a backtick could close the plan's fenced block). Duplicates drop.
fn validate_plan_patterns(field: &str, patterns: &[String]) -> Result<Vec<String>, String> {
    if patterns.len() > CRAWL_PLAN_MAX_PATTERNS {
        return Err(format!(
            "knowledge.crawl_plan {field} takes at most {CRAWL_PLAN_MAX_PATTERNS} patterns"
        ));
    }
    let mut out: Vec<String> = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pattern = pattern.trim();
        if pattern.is_empty() || pattern.chars().count() > CRAWL_PLAN_MAX_PATTERN_CHARS {
            return Err(format!(
                "knowledge.crawl_plan {field} patterns must be 1 to {CRAWL_PLAN_MAX_PATTERN_CHARS} characters"
            ));
        }
        if pattern.chars().any(|c| c.is_control() || c == '`') {
            return Err(format!(
                "knowledge.crawl_plan {field} patterns must not contain control characters or backticks"
            ));
        }
        if !out.iter().any(|seen| seen == pattern) {
            out.push(pattern.to_owned());
        }
    }
    Ok(out)
}

/// Untrusted web or model text made safe for a plan: control characters (newlines included) become a
/// space, backticks are dropped, runs of whitespace collapse, and the result is cut to `max` characters.
fn sanitize_plan_text(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .filter(|c| *c != '`')
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}

/// Whether `url` can sit in a plan: an absolute http(s) URL of sane length with no whitespace, control
/// character or backtick.
fn plan_url_ok(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && url.chars().count() <= CRAWL_PLAN_MAX_URL_CHARS
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '`')
}

fn check_plan_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("knowledge.crawl_plan needs a url".to_owned());
    }
    if !plan_url_ok(url) {
        return Err("knowledge.crawl_plan needs an absolute http(s) url".to_owned());
    }
    Ok(())
}

/// Builds the `knowledge.crawl_plan` answer from a `/v1/map` answer (pure): the pages (at most 50), the
/// validated scope and one fenced `crawl-plan` block the page can turn into an approval card. Nothing
/// has been crawled. The goal and the titles are untrusted web text, so they are capped and stripped of
/// backticks and newlines; a mapped URL that is not a plain absolute http(s) URL is left out.
fn format_crawl_plan(url: &str, goal: &str, map: &Value, scope: &DraftScope) -> String {
    let url = sanitize_plan_text(url, CRAWL_PLAN_MAX_URL_CHARS);
    let goal = sanitize_plan_text(goal, CRAWL_PLAN_MAX_GOAL_CHARS);
    let data = envelope_data(map);
    let links = data
        .get("links")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut pages: Vec<(String, String)> = Vec::new();
    for link in links {
        if pages.len() >= CRAWL_PLAN_MAX_PAGES as usize {
            break;
        }
        let href = link.get("url").and_then(Value::as_str).unwrap_or("").trim();
        if !plan_url_ok(href) || !seen.insert(href.to_owned()) {
            continue;
        }
        let title = sanitize_plan_text(
            link.get("title").and_then(Value::as_str).unwrap_or(""),
            CRAWL_PLAN_MAX_TITLE_CHARS,
        );
        pages.push((href.to_owned(), title));
    }
    if pages.is_empty() {
        return format!(
            "No URLs found for {url}, so no crawl plan was drafted. Nothing has been crawled."
        );
    }
    let max_pages = scope
        .max_pages
        .unwrap_or_else(|| u32::try_from(pages.len()).unwrap_or(u32::MAX))
        .clamp(1, MAX_CRAWL_PAGES);

    let mut scope_json = json!({ "maxPages": max_pages, "allowSubdomains": scope.allow_subdomains });
    if let Some(depth) = scope.max_depth {
        scope_json["maxDepth"] = json!(depth);
    }
    if !scope.include_paths.is_empty() {
        scope_json["includePaths"] = json!(scope.include_paths);
    }
    if !scope.exclude_paths.is_empty() {
        scope_json["excludePaths"] = json!(scope.exclude_paths);
    }
    let page_json: Vec<Value> = pages
        .iter()
        .map(|(href, title)| {
            if title.is_empty() {
                json!({ "url": href })
            } else {
                json!({ "url": href, "title": title })
            }
        })
        .collect();
    let plan = json!({ "v": 1, "url": url, "goal": goal, "pages": page_json, "scope": scope_json });

    let mut out = format!("Crawl plan (nothing has been crawled)\nSite: {url}\nGoal: {goal}\n");
    let _ = writeln!(out, "Pages mapped ({}, at most {CRAWL_PLAN_MAX_PAGES}):", pages.len());
    for (href, title) in &pages {
        if title.is_empty() {
            let _ = writeln!(out, "- {href}");
        } else {
            let _ = writeln!(out, "- {href} ({title})");
        }
    }
    let total = data
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(pages.len());
    let truncated = data
        .get("coverage")
        .and_then(|coverage| coverage.get("truncated"))
        .and_then(Value::as_bool)
        == Some(true);
    if truncated || total > pages.len() {
        out.push_str("(the site has more URLs than are listed)\n");
    }
    let _ = write!(out, "Scope: up to {max_pages} pages");
    match scope.max_depth {
        Some(depth) => {
            let _ = write!(out, ", depth {depth}");
        }
        None => out.push_str(", depth left to Quarry's default"),
    }
    let _ = write!(
        out,
        ", subdomains {}, {} include pattern(s), {} exclude pattern(s).\n\n",
        if scope.allow_subdomains { "allowed" } else { "not allowed" },
        scope.include_paths.len(),
        scope.exclude_paths.len()
    );
    let _ = writeln!(out, "```crawl-plan\n{plan}\n```\n");
    out.push_str(
        "Show this block to the person unchanged; do not call knowledge.crawl_site or knowledge.batch for it; the person approves in the page picker.",
    );
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
    Ok(markdown.to_owned())
}

/// The address an extract reply says the page ended on, or `""` when it names none (pure). The reply carries it on the first
/// result as `final_url`. The edge does not send that field yet, so an absent one is read as the requested URL, and the
/// check passes until it does. An empty answer is read as the requested URL by [`SitePolicy::final_url_refusal`].
fn extract_final_url(value: &Value) -> &str {
    value
        .get("results")
        .and_then(Value::as_array)
        .and_then(|results| results.first())
        .and_then(|result| result.get("final_url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Whether the long-page reader is switched on in this process.
#[must_use]
pub fn reader_enabled() -> bool {
    reader_enabled_from(std::env::var(WEB_READER_ENV).ok().as_deref())
}

/// Only the literal `true` enables; `1`, `on` and anything else stay off.
#[must_use]
pub fn reader_enabled_from(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("true")
}

/// Whether a tool is a page fetch: the one result kind the reader holds for reading.
#[must_use]
pub fn is_page_fetch(tool_name: &str) -> bool {
    matches!(tool_name, "web_fetch" | "web.read")
}

/// The head of a fetched page the loop shows when it does not hold the page: the cut every fetch has
/// always made, so a page the reader cannot hold reads exactly as it did before the reader existed.
#[must_use]
pub fn fetch_head(markdown: &str) -> String {
    truncate_chars(markdown, MAX_FETCH_CHARS)
}

/// The model-facing definitions of `read_page` and `find_in_page`, offered only while the reader is on.
#[must_use]
pub fn reader_tool_definitions() -> Vec<pb::ToolDefinition> {
    if reader_enabled() {
        reader_definitions()
    } else {
        Vec::new()
    }
}

/// The two definitions as the chat reader offers them (model-gateway's `web_reader_tool_definitions`),
/// so the model meets the same names and arguments in either loop.
fn reader_definitions() -> Vec<pb::ToolDefinition> {
    vec![
        pb::ToolDefinition {
            name: READ_PAGE_TOOL.to_owned(),
            description: "Read one section of a long web page that web_fetch held for reading. The section ids come from the page's outline. Each read returns up to 4000 characters, and the result says whether the section continues and the offset to continue from. Read a section you have not read yet rather than fetching the page again. Only pages fetched in this run can be read, and only while their ref is live.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"ref":{"type":"string","description":"The ref from a web_fetch result"},"section":{"type":"string","description":"A section id from the outline, such as s3. Omit to read the outline and the start of the page."},"offset":{"type":"integer","minimum":0,"description":"Characters into the section to start from: the next_offset of the previous read. Defaults to 0."}},"required":["ref"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: FIND_IN_PAGE_TOOL.to_owned(),
            description: "Search a long web page that web_fetch held for reading. Returns the passages that best match the query, each with the section it is in, most relevant first (at most two). Use it when the outline does not show where the answer is, or to check one specific fact on the page. Read the surrounding section with read_page when a passage is not enough.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"ref":{"type":"string","description":"The ref from a web_fetch result"},"query":{"type":"string","description":"The words to look for on the page"}},"required":["ref","query"]}"#.to_owned(),
        },
    ]
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
    fn extract_reads_the_final_url_of_the_first_result_or_none() {
        let landed = json!({"results": [
            {"url": "https://a.example/", "status": "fetched", "markdown": "x", "final_url": "https://b.example/"}
        ]});
        assert_eq!(extract_final_url(&landed), "https://b.example/");
        let silent = json!({"results": [{"url": "https://a.example/", "status": "fetched", "markdown": "x"}]});
        assert_eq!(extract_final_url(&silent), "");
        assert_eq!(extract_final_url(&json!({"results": []})), "");
        assert_eq!(extract_final_url(&json!({})), "");
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
    fn step_attribution_under_zdr_carries_no_run_id() {
        // A step id can carry the run id (the orchestrator mints `{run_id}:step:{n}`), so under ZDR
        // the request id is not built from the step either: neither header names the run.
        let zdr = Attribution::for_step("run_abc123", "run_abc123:step:0", true);
        assert!(zdr.request_id.is_none());
        assert!(zdr.parent_run.is_none());
        let plain = Attribution::for_step("run_abc123", "run_abc123:step:0", false);
        assert_eq!(
            plain.request_id.as_deref(),
            Some("run_abc123.run_abc123:step:0")
        );
        assert_eq!(plain.parent_run.as_deref(), Some("run_abc123"));
    }

    #[test]
    fn the_attribution_switch_is_off_unless_the_literal_true_is_set() {
        assert_eq!(ATTRIBUTION_ENV, "EXECUTION_CORE_QUARRY_ATTRIBUTION");
        assert!(attribution_enabled_from(Some("true")));
        assert!(attribution_enabled_from(Some(" true ")));
        for raw in [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some("on"),
            Some("yes"),
        ] {
            assert!(!attribution_enabled_from(raw), "{raw:?}");
        }
    }

    #[test]
    fn with_the_switch_off_a_step_sends_no_attribution() {
        // Off, a step's Quarry calls carry neither header, under ZDR or not, so they go out as built.
        for zdr in [false, true] {
            let off = Attribution::for_step_when(false, "run_abc123", "run_abc123:step:0", zdr);
            assert!(off.request_id.is_none());
            assert!(off.parent_run.is_none());
        }
        // On, the same step carries the attribution it carried before the switch gated it.
        let on = Attribution::for_step_when(true, "run_abc123", "run_abc123:step:0", false);
        assert_eq!(on.parent_run.as_deref(), Some("run_abc123"));
    }

    #[tokio::test]
    async fn a_zdr_step_sends_no_run_id_to_the_edge() {
        let edge = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": "q", "provider": "hybrid", "results": [], "count": 0
            })))
            .mount(&edge)
            .await;
        let auth = MockServer::start().await;
        mint_mock("org-a", "search:read", minted("search-token"))
            .mount(&auth)
            .await;
        let under_zdr = Attribution::for_step("run_abc123", "run_abc123:step:0", true);
        let client = quarry_client(&edge.uri(), &auth.uri()).with_attribution(under_zdr);
        client
            .search("q", 3, "org-a", true)
            .await
            .expect("the search answers");
        // The same step off ZDR carries both headers, so the absence checked below is a real one.
        let off_zdr = Attribution::for_step("run_abc123", "run_abc123:step:0", false);
        let client = quarry_client(&edge.uri(), &auth.uri()).with_attribution(off_zdr);
        client
            .search("q", 3, "org-a", false)
            .await
            .expect("the search answers");

        let sent = edge.received_requests().await.expect("recorded requests");
        assert_eq!(sent.len(), 2);
        assert!(sent[0].headers.get(REQUEST_ID_HEADER).is_none());
        assert!(sent[0].headers.get(PARENT_RUN_HEADER).is_none());
        assert!(sent[1].headers.get(REQUEST_ID_HEADER).is_some());
        assert!(sent[1].headers.get(PARENT_RUN_HEADER).is_some());
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
        assert_eq!(names, [MAP_TOOL, CRAWL_PLAN_TOOL, CRAWL_SITE_TOOL, BATCH_TOOL]);
        for def in &all {
            let schema: Value = serde_json::from_str(&def.parameters_json)
                .unwrap_or_else(|error| panic!("{}: {error}", def.name));
            assert_eq!(schema["type"], "object", "{}", def.name);
            assert!(is_crawl_tool(&def.name));
        }
        let zdr: Vec<String> = crawl_tool_defs(true).into_iter().map(|def| def.name).collect();
        assert_eq!(zdr, [MAP_TOOL, CRAWL_PLAN_TOOL]);
        assert!(!is_durable_crawl_tool(MAP_TOOL));
        assert!(!is_durable_crawl_tool(CRAWL_PLAN_TOOL));
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

#[cfg(test)]
mod reader_tests {
    use super::*;

    #[test]
    fn the_reader_switch_is_read_only_from_the_literal_true() {
        assert!(reader_enabled_from(Some("true")));
        assert!(reader_enabled_from(Some(" true ")));
        for raw in [None, Some(""), Some("TRUE"), Some("1"), Some("on"), Some("yes")] {
            assert!(!reader_enabled_from(raw), "{raw:?}");
        }
    }

    #[test]
    fn the_definitions_name_the_ref_they_read_by_and_match_the_chat_reader() {
        let defs = reader_definitions();
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();
        assert_eq!(names, ["read_page", "find_in_page"]);
        let read: Value = serde_json::from_str(&defs[0].parameters_json).expect("json schema");
        assert_eq!(read["required"], json!(["ref"]));
        assert!(read["properties"]["section"].is_object());
        let find: Value = serde_json::from_str(&defs[1].parameters_json).expect("json schema");
        assert_eq!(find["required"], json!(["ref", "query"]));
    }

    #[test]
    fn the_head_of_a_page_is_the_cut_every_fetch_has_always_made() {
        let long = "x".repeat(MAX_FETCH_CHARS + 25);
        assert_eq!(fetch_head(&long), truncate_chars(&long, MAX_FETCH_CHARS));
        assert!(fetch_head(&long).ends_with("[truncated 25 chars]"));
        assert_eq!(fetch_head("short page"), "short page");
    }

    #[test]
    fn only_a_page_fetch_is_held_for_reading() {
        assert!(is_page_fetch("web_fetch"));
        assert!(is_page_fetch("web.read"));
        assert!(!is_page_fetch("web_search"));
        assert!(!is_page_fetch(READ_PAGE_TOOL));
    }

    #[test]
    fn an_extracted_page_comes_back_whole_for_the_loop_to_cut_or_hold() {
        let markdown = "y".repeat(MAX_FETCH_CHARS + 10);
        let value = json!({
            "results": [{"url": "https://x", "status": "fetched", "markdown": markdown}]
        });
        let whole = extract_markdown("https://x", &value).expect("markdown");
        assert_eq!(whole.chars().count(), MAX_FETCH_CHARS + 10);
    }
}

// ---- EP-9 W3: the org's domain rules before a web call (see `site_policy_client`) ---------------------------

#[cfg(test)]
mod site_policy_tests {
    use std::sync::Arc;

    use mp_contracts::site_policy::WebDomainPolicyMode;
    use serde_json::{json, Value};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::tests::{mint_mock, minted, quarry_client};
    use crate::site_policy_client::{SiteChecker, SiteOwner, SitePolicy};

    const ORG: &str = "org-a";

    /// org-core's resolve route for `ORG`, answering with `decisions`.
    async fn org_core(decisions: Value) -> MockServer {
        let org = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org-a/browser-sites/resolve"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-token", "org-core-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "organization_id": ORG,
                "revision": 3,
                "decisions": decisions,
            })))
            .mount(&org)
            .await;
        org
    }

    /// The policy in `mode` against `org`, with a fresh cache.
    fn under(mode: WebDomainPolicyMode, org: &MockServer) -> SitePolicy {
        SitePolicy::new(
            mode,
            SiteOwner::new(&org.uri(), "execution-core", "org-core-test-token"),
            Arc::new(SiteChecker::default()),
        )
    }

    /// The policy in enforcement against `org`, with a fresh cache.
    fn enforcing(org: &MockServer) -> SitePolicy {
        under(WebDomainPolicyMode::Enforce, org)
    }

    fn blocked_and_open() -> Value {
        json!([
            {"host": "example.org", "effective": "deny", "cap": "none", "source": "org_block"},
            {"host": "example.com", "effective": "ask", "cap": "act", "source": "default"}
        ])
    }

    #[tokio::test]
    async fn a_fetch_of_a_blocked_host_is_refused_before_quarry_is_called() {
        let org = org_core(blocked_and_open()).await;
        let edge = MockServer::start().await;
        // The extract route must never be reached: the refusal comes before any call to the edge.
        Mock::given(method("POST"))
            .and(path("/v1/extract"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&edge)
            .await;
        let mut client = quarry_client(&edge.uri(), "http://auth.invalid");
        client.site = enforcing(&org);

        let refusal = client
            .fetch("https://example.org/page", ORG, false)
            .await
            .expect_err("a blocked host is refused");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(refusal.contains("'example.org'"), "{refusal}");
        assert!(refusal.contains("'web_fetch'"), "{refusal}");
    }

    #[tokio::test]
    async fn a_search_shows_no_hit_on_a_host_the_org_does_not_allow() {
        let org = org_core(blocked_and_open()).await;
        let edge = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": "norge",
                "provider": "brave",
                "results": [
                    {"url": "https://example.org/a", "title": "Blocked hit", "snippet": "s", "rank": 1},
                    {"url": "https://example.com/b", "title": "Open hit", "snippet": "t", "rank": 2}
                ],
                "count": 2
            })))
            .mount(&edge)
            .await;
        let auth = MockServer::start().await;
        mint_mock(ORG, "search:read", minted("search-token"))
            .mount(&auth)
            .await;
        let mut client = quarry_client(&edge.uri(), &auth.uri());
        client.site = enforcing(&org);

        let shown = client
            .search("norge", 5, ORG, false)
            .await
            .expect("the search answers");
        assert!(shown.contains("https://example.com/b"), "{shown}");
        assert!(!shown.contains("example.org"), "{shown}");
    }

    #[tokio::test]
    async fn a_map_or_a_crawl_of_a_blocked_start_host_is_refused_before_quarry_is_called() {
        let org = org_core(blocked_and_open()).await;
        let edge = MockServer::start().await;
        for route in ["/v1/map", "/v1/crawl", "/v1/batch"] {
            Mock::given(method("POST"))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&edge)
                .await;
        }
        let mut client = quarry_client(&edge.uri(), "http://auth.invalid");
        client.site = enforcing(&org);

        assert!(client
            .map_site("https://example.org/", None, 10, false, ORG)
            .await
            .is_err_and(|refusal| refusal.starts_with("org_site_blocked: ")));
        assert!(client
            .start_crawl("https://example.org/", 10, None, false, ORG)
            .await
            .is_err_and(|refusal| refusal.starts_with("org_site_blocked: ")));
        assert!(client
            .start_batch(
                &["https://example.com/a".to_owned(), "https://example.org/b".to_owned()],
                false,
                ORG,
            )
            .await
            .is_err_and(|refusal| refusal.contains("'example.org'")));
    }

    /// The text of the page a fetch of `REQUESTED` reads, so a test can see whether it reached the model.
    const LANDED: &str = "landed page text LANDED-MARKER-7f3a";
    const REQUESTED: &str = "https://example.com/start";

    /// The extract reply for `REQUESTED`: the page's markdown, and the address it ended on when the edge names one.
    fn extract_reply(final_url: Option<&str>) -> Value {
        let mut item = json!({"url": REQUESTED, "status": "fetched", "markdown": LANDED});
        if let Some(final_url) = final_url {
            item["final_url"] = json!(final_url);
        }
        json!({"results": [item], "count": 1, "requested": 1})
    }

    /// `web_fetch` of `REQUESTED` against a quarry-edge that answers `reply`, with the org's rules in `mode`.
    async fn fetch_through(
        mode: WebDomainPolicyMode,
        org: &MockServer,
        reply: Value,
    ) -> Result<String, String> {
        let edge = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/extract"))
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .mount(&edge)
            .await;
        let auth = MockServer::start().await;
        mint_mock(ORG, "extract:read", minted("extract-token"))
            .mount(&auth)
            .await;
        let mut client = quarry_client(&edge.uri(), &auth.uri());
        client.site = under(mode, org);
        client.fetch(REQUESTED, ORG, false).await
    }

    async fn asked(org: &MockServer) -> usize {
        org.received_requests().await.unwrap_or_default().len()
    }

    #[tokio::test]
    async fn a_redirect_onto_a_blocked_host_is_refused_under_enforce() {
        let org = org_core(blocked_and_open()).await;
        let reply = extract_reply(Some("https://example.org/landing?q=private-query"));
        let refusal = fetch_through(WebDomainPolicyMode::Enforce, &org, reply)
            .await
            .expect_err("a page that ended on a blocked host is refused");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(refusal.contains("'example.org'"), "{refusal}");
        assert!(refusal.contains("'web_fetch'"), "{refusal}");
        assert!(
            !refusal.contains(LANDED),
            "the page text is not returned: {refusal}"
        );
        assert!(
            !refusal.contains("private-query"),
            "no query in the refusal: {refusal}"
        );
    }

    #[tokio::test]
    async fn a_redirect_onto_a_blocked_host_is_read_under_shadow_and_off() {
        for mode in [WebDomainPolicyMode::Shadow, WebDomainPolicyMode::Off] {
            let org = org_core(blocked_and_open()).await;
            let reply = extract_reply(Some("https://example.org/landing"));
            let text = fetch_through(mode, &org, reply)
                .await
                .unwrap_or_else(|error| panic!("{mode:?}: {error}"));
            assert!(text.contains(LANDED), "{mode:?}: {text}");
            // The requested host is asked under both. The landed host is asked only where the rules are read.
            let expected = if mode == WebDomainPolicyMode::Off {
                0
            } else {
                2
            };
            assert_eq!(asked(&org).await, expected, "{mode:?}");
        }
    }

    #[tokio::test]
    async fn an_empty_or_absent_final_url_is_the_requested_url_and_asks_org_core_only_for_it() {
        for reply in [
            extract_reply(None),
            extract_reply(Some("")),
            extract_reply(Some("   ")),
            extract_reply(Some("https://EXAMPLE.com/other")),
        ] {
            let org = org_core(blocked_and_open()).await;
            let text = fetch_through(WebDomainPolicyMode::Enforce, &org, reply)
                .await
                .expect("the requested page is read");
            assert!(text.contains(LANDED), "{text}");
            assert_eq!(asked(&org).await, 1, "only the requested host is asked");
        }
    }

    #[tokio::test]
    async fn a_redirect_onto_another_allowed_host_is_read_under_enforce() {
        let org = org_core(json!([
            {"host": "example.com", "effective": "ask", "cap": "act", "source": "default"},
            {"host": "example.net", "effective": "read", "cap": "read", "source": "default"}
        ]))
        .await;
        let reply = extract_reply(Some("https://example.net/landing"));
        let text = fetch_through(WebDomainPolicyMode::Enforce, &org, reply)
            .await
            .expect("a landing on an allowed host is read");
        assert!(text.contains(LANDED), "{text}");
    }

    #[tokio::test]
    async fn a_final_url_with_no_host_is_refused_under_enforce() {
        let org = org_core(blocked_and_open()).await;
        let refusal = fetch_through(
            WebDomainPolicyMode::Enforce,
            &org,
            extract_reply(Some("not a url")),
        )
        .await
        .expect_err("a page that ended where no host can be named is refused");
        assert!(refusal.contains("cannot be checked"), "{refusal}");
    }

    #[tokio::test]
    async fn a_blocked_landing_is_named_even_when_the_reply_carries_no_text() {
        let org = org_core(blocked_and_open()).await;
        let reply = json!({"results": [{
            "url": REQUESTED,
            "status": "blocked",
            "markdown": "",
            "final_url": "https://example.org/landing"
        }], "count": 1, "requested": 1});
        let refusal = fetch_through(WebDomainPolicyMode::Enforce, &org, reply)
            .await
            .expect_err("the landed host is refused before the empty reply is read");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
    }
}
