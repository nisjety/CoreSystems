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

use crate::quarry_auth::TokenProvider;

const SCRAPE_SCOPES: &[&str] = &["scrape:read"];
const SEARCH_SCOPES: &[&str] = &["search:read"];

/// How long the browser driver may wait for client-rendered markup to
/// settle on the one escalated retry. Comfortably inside the 30s scrape
/// timeout so the wait can never be what times the call out.
const RENDER_SETTLE_MS: u32 = 2_500;

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

    /// Auth Core could not mint a bounded Quarry credential.
    #[error("quarry: authentication failed: {0}")]
    Authentication(String),
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
    ///
    /// Both fields count. Testing only `wait_for_selector` silently
    /// dropped timeout-only hints, so a caller that asked for "settle
    /// for 2s" got no wait at all.
    fn has_any(&self) -> bool {
        self.wait_for_selector.is_some() || self.wait_for_timeout_ms.is_some()
    }
}

/// Quarry's driver-selection signals — the field that actually decides
/// static vs TLS vs browser. `render` does NOT influence that choice:
/// `quarry-edge` calls `plan_from_signals(req.signals.unwrap_or_default())`
/// and only *then* hands `render` to the chosen driver, so render hints
/// on a default-signalled request are applied by a driver that never
/// executes JavaScript.
///
/// Mirrors `quarry_runtime::driver_plan::DriverSignals`. That struct
/// derives `Deserialize` **without** `#[serde(default)]` on the struct
/// or its fields, so once `signals` is present on the wire every field
/// is mandatory — omitting one makes the edge reject the whole request.
/// Every field is therefore always serialised.
#[derive(Debug, Clone, Serialize)]
pub struct DriverSignals {
    /// Browser actions to perform. Non-empty forces the browser driver.
    pub actions: Vec<String>,
    pub screenshot: bool,
    pub pdf: bool,
    /// Prior blocking signals for this origin. `>= 2` selects the
    /// browser driver, `== 1` selects the TLS-profile driver.
    pub prior_block_signals: u8,
    pub profile_required: bool,
    /// Serialised form of Quarry's `UrlType` enum (externally tagged
    /// unit variants → a bare string). `"Default"` is its `#[default]`.
    pub url_type: &'static str,
}

impl Default for DriverSignals {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            screenshot: false,
            pdf: false,
            prior_block_signals: 0,
            profile_required: false,
            url_type: "Default",
        }
    }
}

impl DriverSignals {
    /// Signals that make Quarry's planner pick the **browser** driver.
    ///
    /// Uses `prior_block_signals = 2`, the documented escalation
    /// threshold in `quarry-runtime/src/driver_plan.rs`: a static fetch
    /// that yields no readable text is exactly the soft block that
    /// threshold exists for. Deliberately does not set `screenshot` /
    /// `pdf` / `actions`, which would also force a browser but bill an
    /// artifact we do not want.
    #[must_use]
    pub fn browser() -> Self {
        Self {
            prior_block_signals: 2,
            ..Self::default()
        }
    }
}

/// Per-call scrape knobs. Added as a struct rather than more positional
/// parameters so new Quarry request fields don't churn every call site.
#[derive(Debug, Clone, Default)]
pub struct ScrapeOptions {
    pub render: Option<RenderHints>,
    pub signals: Option<DriverSignals>,
    pub prefer_http3: bool,
    pub zdr: bool,
}

impl ScrapeOptions {
    /// The one retry [`Client::scrape_readable`] makes when a plain
    /// fetch came back with no readable text: browser driver plus a
    /// render wait so client-rendered markup exists before extraction.
    ///
    /// `wait_for_selector` is required — [`RenderHints::has_any`] drops
    /// a hint that only carries a timeout, and `body` is present on
    /// every HTML document, so it costs nothing on pages that are
    /// already settled.
    #[must_use]
    pub fn browser_escalation(zdr: bool) -> Self {
        Self {
            render: Some(RenderHints {
                wait_for_selector: Some("body".to_owned()),
                wait_for_timeout_ms: Some(RENDER_SETTLE_MS),
            }),
            signals: Some(DriverSignals::browser()),
            prefer_http3: false,
            zdr,
        }
    }
}

/// Per-call search knobs: the subset of `quarry-edge`'s `SearchRequest`
/// (`quarry-edge/src/search_routes.rs`) the Model Plane has a caller for.
/// Added as a struct rather than more positional parameters for the same
/// reason as [`ScrapeOptions`].
///
/// The field names below are the edge's own and must stay spelled that way:
/// its `SearchRequest` derives `Deserialize` without `deny_unknown_fields`,
/// so a misspelled key is dropped in silence on arrival. The failure mode of
/// a typo here is therefore not an error anyone sees — it is a Norwegian
/// question searched with an English bias, exactly the bug these fields
/// exist to fix.
///
/// Every *narrowing* field is optional and is omitted from the request body
/// when unset, so a call that supplies no options sends no query-shaping keys
/// at all and cannot narrow a result set against an edge build that predates
/// them. [`Self::allow_paid_providers`] is the deliberate exception and is
/// always on the wire — see its own note for why that is worth the one key.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Query language, as a bare code or a full locale (`"nb"`, `"nb-NO"`).
    /// Deliberately not folded to one form here — each provider wants a
    /// different one (Brave answers 422 to `"nb-NO"`, `SearXNG` wants the full
    /// locale) and `quarry-runtime`'s `serp` module already normalises per
    /// provider, so a second normalisation here could only disagree with it.
    pub language: Option<String>,
    /// Region bias, as alpha-2 or a full locale (`"NO"`, `"nb-NO"`).
    pub country: Option<String>,
    /// Topic vertical: `"general"`, `"news"` or `"finance"`.
    pub topic: Option<String>,
    /// Recency window: `"day"`, `"week"`, `"month"` or `"year"`. The edge
    /// lowercases this and validates it against that closed set, dropping
    /// anything else, so an unrecognised value degrades to "no window"
    /// rather than to a rejected request.
    pub time_range: Option<String>,
    /// Restrict results to these domains (applied as `site:` operators).
    /// Empty = no restriction.
    pub include_domains: Vec<String>,
    /// Exclude these domains (applied as `-site:` operators). Empty = no
    /// exclusion.
    pub exclude_domains: Vec<String>,
    /// Whether the edge may reach a paid, externally-egressing provider
    /// (Brave) for this call. `false` — the [`Default`] — leaves `SearXNG`,
    /// which every tier gets.
    ///
    /// Unlike every field above this one is **always serialised**. The edge
    /// treats an absent field as "no paid providers", so skipping it when
    /// false would be correct-by-accident and unreadable on the wire: a
    /// captured request body would not distinguish "this tenant is not
    /// entitled" from "this client is too old to know about tiering". One
    /// always-present boolean makes the grant self-describing in logs and in
    /// the edge's own request records, which is what an egress decision needs
    /// to be auditable after the fact.
    pub allow_paid_providers: bool,
}

/// Trimmed value, or `None` when the caller supplied nothing usable.
///
/// These options originate in model-authored tool arguments, which routinely
/// carry `""`. A blank is not merely equivalent to absent: the edge's search
/// cache key is built from the *option* (`lg=Some("")` vs `lg=None`), so
/// sending blanks would split otherwise identical queries across separate
/// cache entries while changing no result.
fn non_blank(value: Option<&String>) -> Option<&str> {
    value.map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// The non-blank entries of a domain filter list. Same cache-key reasoning
/// as [`non_blank`]: `[""]` and `[]` are distinct keys upstream.
fn non_blank_domains(values: &[String]) -> Vec<&str> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect()
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
    /// Relevance score 0.0–1.0, **0.0 when Quarry sent none**. Kept as a bare
    /// `f32` because it feeds a non-optional proto field; anything deciding
    /// whether the hit was actually judged must read [`Self::relevance`]
    /// instead, where absent and zero are different values.
    pub score: f32,
    /// Quarry's semantic-reranker relevance in `[0,1]`, or `None` when this hit
    /// was not reranked.
    ///
    /// Quarry wraps its search provider in a `RerankingSearchProvider` that asks
    /// the Model Plane to score how well each of the leading results answers the
    /// query, and omits the field entirely otherwise — reranking is off for the
    /// deployment, the Model Plane call failed (the reranker degrades to the
    /// original order), or the hit fell outside the reranked head. Absent is
    /// therefore "not judged", which is a different fact from "judged
    /// irrelevant": collapsing the two into `0.0` (as [`Self::score`] must, for
    /// the proto) would let an unreranked response filter itself to nothing.
    /// [`crate::relevance`] depends on this distinction.
    pub relevance: Option<f32>,
    /// Provider-supplied SERP position, 1-based, `0` when Quarry sent none.
    /// Lower is better. Always present in practice; the reranker renumbers it
    /// after reordering, so it reflects the order the caller actually received.
    pub rank: u32,
    /// Query-relevant highlight passages attached by the reranker (Exa-style).
    /// Empty when the hit was not reranked. Better evidence of *why* a hit
    /// matched than a provider snippet, which is often boilerplate.
    pub highlights: Vec<String>,
    /// The upstream engines that returned this URL, as Quarry's metasearch saw
    /// them. Empty until the Ingestion Plane side ships the field.
    ///
    /// Cross-engine agreement is the main quality signal a metasearch has —
    /// one URL returned by four independent engines is a stronger hit than one
    /// engine's top result — and it is a signal no single provider's own score
    /// can express. Projected now, ahead of any consumer, so that the day the
    /// edge starts sending it the data is already here rather than being
    /// dropped on the floor by an older gateway.
    pub engines: Vec<String>,
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
    /// Static bearer used only by isolated tests. Production constructs the
    /// client with [`Client::from_env`] and never reads a static Quarry token.
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
    auth: Auth,
    http: reqwest::Client,
}

#[derive(Clone)]
enum Auth {
    Static(String),
    ServicePrincipal(TokenProvider),
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static(_) => formatter.write_str("Static([REDACTED])"),
            Self::ServicePrincipal(provider) => formatter
                .debug_tuple("ServicePrincipal")
                .field(provider)
                .finish(),
        }
    }
}

impl Client {
    /// Build a client. Returns the empty form (which reports
    /// `Available() == false`) when `cfg.base_url` is empty.
    #[must_use]
    pub fn new(cfg: Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            // Practically unreachable for the trivial config above;
            // fall back to a default client so construction is
            // infallible (callers don't have to thread a Result through
            // AppState initialisation).
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            auth: Auth::Static(cfg.token),
            http,
        }
    }

    /// Build the production client backed by Auth Core service-principal
    /// token minting. No static Quarry bearer is read or retained.
    ///
    /// # Errors
    ///
    /// Returns an authentication/configuration error when Auth Core or the
    /// dedicated model-gateway service credential is unavailable.
    pub fn from_env(base_url: &str, timeout: Duration) -> Result<Self, QuarryError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(QuarryError::Transport)?;
        let provider = TokenProvider::from_env()
            .map_err(|error| QuarryError::Authentication(error.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth: Auth::ServicePrincipal(provider),
            http,
        })
    }

    async fn token(&self, org_id: &str, scopes: &[&str]) -> Result<String, QuarryError> {
        match &self.auth {
            Auth::Static(token) => Ok(token.clone()),
            Auth::ServicePrincipal(provider) => provider
                .token(org_id, scopes)
                .await
                .map_err(|error| QuarryError::Authentication(error.to_string())),
        }
    }

    async fn invalidate_if_matches(
        &self,
        org_id: &str,
        scopes: &[&str],
        rejected_token: &str,
    ) -> Result<(), QuarryError> {
        match &self.auth {
            Auth::Static(_) => Ok(()),
            Auth::ServicePrincipal(provider) => provider
                .invalidate_if_matches(org_id, scopes, rejected_token)
                .await
                .map_err(|error| QuarryError::Authentication(error.to_string())),
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
        zdr: bool,
    ) -> Result<ScrapeResult, QuarryError> {
        self.scrape_with_options(
            url,
            org_id,
            &ScrapeOptions {
                render: render.cloned(),
                signals: None,
                prefer_http3,
                zdr,
            },
        )
        .await
    }

    /// Fetch a page and return its readable text, escalating to a
    /// rendered fetch **once** when the plain fetch yields nothing.
    ///
    /// This is the entry point every page-read caller should use. The
    /// fast path stays one static fetch: the browser is only paid for
    /// when the cheap path demonstrably produced no text.
    ///
    /// Escalation happens **only** for an empty body — never for an
    /// error (a 403/timeout is not fixed by rendering, and retrying it
    /// would double the cost of every genuinely broken URL) and never
    /// more than once. If the rendered retry is also empty (or itself
    /// fails) the first result is returned unchanged, so the caller
    /// still reports the page as unread rather than inventing content.
    ///
    /// # Errors
    ///
    /// Propagates the first fetch's [`QuarryError`]. A failure of the
    /// escalated retry is swallowed in favour of the first result.
    pub async fn scrape_readable(
        &self,
        url: &str,
        org_id: &str,
        zdr: bool,
    ) -> Result<ScrapeResult, QuarryError> {
        let first = self
            .scrape_with_options(
                url,
                org_id,
                &ScrapeOptions {
                    zdr,
                    ..ScrapeOptions::default()
                },
            )
            .await?;
        if !first.text.trim().is_empty() {
            return Ok(first);
        }

        tracing::debug!(
            %url,
            status = first.status,
            "quarry scrape returned no readable text; escalating once to the browser driver"
        );
        match self
            .scrape_with_options(url, org_id, &ScrapeOptions::browser_escalation(zdr))
            .await
        {
            Ok(rendered) if !rendered.text.trim().is_empty() => {
                tracing::debug!(
                    %url,
                    chars = rendered.text.chars().count(),
                    "rendered retry recovered readable text"
                );
                Ok(rendered)
            }
            Ok(_) => {
                tracing::debug!(%url, "rendered retry also returned no readable text");
                Ok(first)
            }
            Err(error) => {
                tracing::debug!(%url, %error, "rendered retry failed; keeping the empty result");
                Ok(first)
            }
        }
    }

    /// POST `/v1/scrape` with explicit [`ScrapeOptions`] and project the
    /// response, resolving any artifact-referenced text.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] if the edge is unavailable, the URL is empty,
    /// the upstream returns a non-2xx status, or the response cannot be decoded.
    pub async fn scrape_with_options(
        &self,
        url: &str,
        org_id: &str,
        options: &ScrapeOptions,
    ) -> Result<ScrapeResult, QuarryError> {
        #[derive(Serialize)]
        struct Body<'a> {
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            render: Option<&'a RenderHints>,
            /// Omitted unless the caller wants a non-default driver, so
            /// an ordinary fetch keeps the exact wire shape it had
            /// before signals existed.
            #[serde(skip_serializing_if = "Option::is_none")]
            signals: Option<&'a DriverSignals>,
            #[serde(skip_serializing_if = "is_false", rename = "prefer_http3")]
            prefer_http3: bool,
            zdr: bool,
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
            render: options.render.as_ref().filter(|r| r.has_any()),
            signals: options.signals.as_ref(),
            prefer_http3: options.prefer_http3,
            zdr: options.zdr,
        };

        let endpoint = format!("{}/v1/scrape", self.base_url);
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id, SCRAPE_SCOPES).await?;
            let mut req = self.http.post(&endpoint).json(&body);
            if !token.is_empty() {
                req = req.bearer_auth(&token);
            }
            if !org_id.is_empty() {
                req = req.header("X-Quarry-Org", org_id);
            }
            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, SCRAPE_SCOPES, &token)
                    .await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
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

        let data = Value::Object(data.clone());
        let mut result = project(url, &data);

        // Quarry does not inline page text. `OutputFormats.markdown` /
        // `.html` are `Option<FormatRef>` — `{artifact_id, bytes}` — and
        // there is no `text` field at all, so `project`'s string reads
        // are empty for every real page. Resolve the reference to get
        // the bytes the fetch already produced.
        //
        // Skipped under ZDR: on a zero-retention turn Quarry emits no
        // formats to begin with, and pulling persisted bytes back would
        // defeat the guarantee even if it did.
        if result.text.trim().is_empty() && !options.zdr {
            if let Some(text) = self.referenced_markdown(url, org_id, &data).await {
                result.markdown = text.clone();
                result.text = text;
            }
        }

        Ok(result)
    }

    /// Resolve `formats.markdown`'s artifact reference into text, or
    /// `None` when there is no reference, it is empty, or the read is
    /// refused. A refusal is logged rather than raised: the page is then
    /// reported unread, which is honest, instead of looking like a page
    /// that genuinely had no words in it.
    async fn referenced_markdown(&self, url: &str, org_id: &str, data: &Value) -> Option<String> {
        let reference = artifact_ref(data, "markdown")?;
        match self.artifact_text(&reference.artifact_id, org_id).await {
            Ok(text) if !text.trim().is_empty() => Some(text),
            Ok(_) => {
                tracing::debug!(
                    %url,
                    artifact_id = %reference.artifact_id,
                    "quarry markdown artifact resolved to an empty body"
                );
                None
            }
            Err(error) => {
                tracing::warn!(
                    %url,
                    artifact_id = %reference.artifact_id,
                    bytes = reference.bytes,
                    %error,
                    "quarry returned page text as an artifact reference that could not be \
                     resolved; the page will be reported as unread"
                );
                None
            }
        }
    }

    /// GET `/v1/artifacts/{id}` and return the body as UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] when the edge is unavailable, rejects the
    /// read, or the artifact is not valid UTF-8.
    async fn artifact_text(&self, artifact_id: &str, org_id: &str) -> Result<String, QuarryError> {
        if !self.available() {
            return Err(QuarryError::Unavailable);
        }
        let endpoint = format!("{}/v1/artifacts/{artifact_id}", self.base_url);
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id, SCRAPE_SCOPES).await?;
            let mut req = self.http.get(&endpoint);
            if !token.is_empty() {
                req = req.bearer_auth(&token);
            }
            if !org_id.is_empty() {
                req = req.header("X-Quarry-Org", org_id);
            }
            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, SCRAPE_SCOPES, &token)
                    .await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        let status = resp.status().as_u16();
        let raw = resp.text().await?;
        if status >= 400 {
            return Err(QuarryError::Typed {
                code: format!("HTTP_{status}"),
                message: truncate(&raw, 200),
                status,
            });
        }
        Ok(raw)
    }

    /// Call `/v1/search` with no per-call options and return the projected
    /// results. Equivalent to [`Client::search_with_options`] with a default
    /// [`SearchOptions`], and kept as its own entry point so existing callers
    /// stay unchanged.
    ///
    /// The edge's `SmartSearchRouter` picks the provider (local Tantivy,
    /// Tavily, Bing, Google). `intent` is a **hint only**: the edge accepts
    /// and logs it, but its router reaches the handler as
    /// `Arc<dyn SearchProvider>`, whose `search(query, &SearchOptions)` has
    /// no intent slot — so routing is still re-derived from the query text.
    /// Values are matched against the router's `QueryIntent` vocabulary
    /// (`navigational`, `fresh`/`news`/`recent`, `phrase`/`exact`,
    /// `research`, `comparative`/`compare`, `local`, `code`,
    /// `default`/`general`); anything else is logged and dropped there
    /// rather than rejected, so an unknown value costs nothing but is also
    /// worth nothing.
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
        zdr: bool,
    ) -> Result<Vec<SearchResult>, QuarryError> {
        self.search_with_options(
            query,
            limit,
            intent,
            org_id,
            zdr,
            &SearchOptions::default(),
        )
        .await
    }

    /// Call `/v1/search` with explicit [`SearchOptions`] — language, region,
    /// topic vertical, recency window and domain filters.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] if the edge is unavailable, the query is empty,
    /// the upstream returns a non-2xx status, or the response cannot be decoded.
    pub async fn search_with_options(
        &self,
        query: &str,
        limit: i32,
        intent: &str,
        org_id: &str,
        zdr: bool,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>, QuarryError> {
        #[derive(Serialize)]
        struct Body<'a> {
            query: &'a str,
            limit: i32,
            #[serde(skip_serializing_if = "str::is_empty")]
            intent: &'a str,
            zdr: bool,
            /// Never skipped — see [`SearchOptions::allow_paid_providers`].
            /// Grouped with `zdr` rather than with the narrowing options below
            /// because, like `zdr`, it is a standing property of the caller
            /// that belongs on every request, not a per-query refinement.
            allow_paid_providers: bool,
            // Everything below is additive and declared last so it is skipped
            // wholesale under a default `SearchOptions`: the body then carries
            // no query-shaping keys at all, which is what lets `search` stay a
            // safe passthrough against an edge deployment that has not shipped
            // these fields.
            #[serde(skip_serializing_if = "Option::is_none")]
            language: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            country: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            topic: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            time_range: Option<&'a str>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            include_domains: Vec<&'a str>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            exclude_domains: Vec<&'a str>,
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
            zdr,
            allow_paid_providers: options.allow_paid_providers,
            language: non_blank(options.language.as_ref()),
            country: non_blank(options.country.as_ref()),
            topic: non_blank(options.topic.as_ref()),
            time_range: non_blank(options.time_range.as_ref()),
            include_domains: non_blank_domains(&options.include_domains),
            exclude_domains: non_blank_domains(&options.exclude_domains),
        };

        let endpoint = format!("{}/v1/search", self.base_url);
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id, SEARCH_SCOPES).await?;
            let mut req = self.http.post(&endpoint).json(&body);
            if !token.is_empty() {
                req = req.bearer_auth(&token);
            }
            if !org_id.is_empty() {
                req = req.header("X-Quarry-Org", org_id);
            }
            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, SEARCH_SCOPES, &token)
                    .await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
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

/// Read the result rows out of a `/v1/search` response body.
///
/// Top-level `results` is checked FIRST because it is the only shape the edge
/// actually produces: `quarry-edge`'s search handler returns
/// `Json(SearchResponse)` — `{query, provider, results, count, ...}` — and the
/// route carries no response-wrapping middleware (auth, trace, body-limit and
/// timeout layers only). The `data` envelope is `/v1/scrape`'s, built by that
/// handler itself, which is where the reversed order here came from; it left
/// the live search path running entirely on what was written as a fallback.
///
/// The `data` branch is kept, not deleted: the Frontend Plane BFF wraps
/// upstream payloads in `{data: ...}`, so a body that has been through it
/// still parses. model-gateway does not call search through the BFF today —
/// this is compatibility, not a supported second contract.
fn extract_search_results(payload: &Value) -> Vec<Value> {
    payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            payload
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("results"))
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default()
}

// reason: provider scores are small; i64/f64→f32 loses no meaningful precision
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn project_search_result(v: &Value) -> SearchResult {
    let obj = v.as_object().cloned().unwrap_or_default();
    let source = string_field(&obj, "source");
    // Score may be float or int depending on provider; coerce both. Absent (or
    // non-finite) stays `None` — see `SearchResult::relevance` for why that is
    // load-bearing rather than pedantry.
    let relevance = obj
        .get("score")
        .and_then(|s| s.as_f64().or_else(|| s.as_i64().map(|n| n as f64)))
        .map(|f| f as f32)
        .filter(|f| f.is_finite());
    SearchResult {
        url: string_field(&obj, "url"),
        title: string_field(&obj, "title"),
        snippet: string_field(&obj, "snippet"),
        source: if source.is_empty() {
            string_field(&obj, "provider")
        } else {
            source
        },
        score: relevance.unwrap_or(0.0),
        relevance,
        rank: obj
            .get("rank")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        highlights: string_array(&obj, "highlights"),
        engines: string_array(&obj, "engines"),
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

/// One of Quarry's `FormatRef`s: page bytes held in the artifact store
/// rather than inlined in the scrape response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub bytes: u64,
}

/// Read `data.formats[key]` as a `FormatRef`. Returns `None` when the
/// key is absent or is an inline string (tolerated so the projection
/// keeps working if Quarry ever starts inlining small documents).
fn artifact_ref(data: &Value, key: &str) -> Option<ArtifactRef> {
    let entry = data.get("formats")?.as_object()?.get(key)?.as_object()?;
    let artifact_id = entry.get("artifact_id")?.as_str()?.to_owned();
    if artifact_id.is_empty() {
        return None;
    }
    Some(ArtifactRef {
        artifact_id,
        bytes: entry.get("bytes").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// The non-blank string entries of `obj[key]`, or an empty vec when the key is
/// absent or is not an array. Blanks are dropped rather than carried: an empty
/// highlight is not evidence and an unnamed engine is not a vote, so keeping
/// them would only inflate whatever counts them downstream.
fn string_array(obj: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    obj.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    #[derive(Clone)]
    struct RotatingTokenResponse(Arc<AtomicUsize>);

    impl Respond for RotatingTokenResponse {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let token = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                "expired-token"
            } else {
                "fresh-token"
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": token,
                "expiresInSeconds": 300,
                "audience": "quarry"
            }))
        }
    }

    #[test]
    fn empty_base_url_is_unavailable() {
        let c = Client::new(Config::default());
        assert!(!c.available());
    }

    #[tokio::test]
    async fn unavailable_client_returns_unavailable_error() {
        let c = Client::new(Config::default());
        let err = c
            .scrape("https://example.com", "", None, false, false)
            .await
            .expect_err("must error");
        assert!(matches!(err, QuarryError::Unavailable));
    }

    #[tokio::test]
    async fn scrape_retries_exactly_once_after_unauthorized() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&quarry)
            .await;
        let client = Client::new(Config {
            base_url: quarry.uri(),
            token: "expired".to_owned(),
            timeout: Duration::from_secs(5),
        });

        let error = client
            .scrape("https://example.com", "org-a", None, false, false)
            .await
            .expect_err("second 401 must be surfaced");
        assert!(matches!(error, QuarryError::Typed { status: 401, .. }));
    }

    #[tokio::test]
    async fn scrape_forwards_zero_data_retention() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://example.com",
                "zdr": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"status": 200, "formats": {"text": "ok"}}
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        let client = Client::new(Config {
            base_url: quarry.uri(),
            token: "test-token".to_owned(),
            timeout: Duration::from_secs(5),
        });

        client
            .scrape("https://example.com", "org-a", None, false, true)
            .await
            .expect("ZDR scrape succeeds");
    }

    #[tokio::test]
    async fn unauthorized_dynamic_token_is_refreshed_before_retry() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/quarry/internal-token"))
            .respond_with(RotatingTokenResponse(Arc::new(AtomicUsize::new(0))))
            .expect(2)
            .mount(&auth)
            .await;
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(header("authorization", "Bearer expired-token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(header("authorization", "Bearer fresh-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"status": 200, "formats": {"text": "ok"}}
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        let provider = TokenProvider::new_for_test(auth.uri(), "model-gateway", "secret");
        let client = Client {
            base_url: quarry.uri(),
            auth: Auth::ServicePrincipal(provider),
            http: reqwest::Client::new(),
        };

        let result = client
            .scrape("https://example.com", "org-a", None, false, false)
            .await
            .expect("fresh token retry succeeds");
        assert_eq!(result.text, "ok");
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
    fn render_hints_has_any_accepts_a_timeout_only_hint() {
        // A timeout-only hint used to be dropped on the floor, so
        // "settle for 2s" silently became "no wait at all".
        assert!(RenderHints {
            wait_for_selector: None,
            wait_for_timeout_ms: Some(2_000),
        }
        .has_any());
    }

    /// Quarry's `DriverSignals` has no `#[serde(default)]`, so every
    /// field must be on the wire or the edge rejects the request. This
    /// pins the exact field names and the browser-selecting values.
    #[test]
    fn browser_signals_serialize_to_quarrys_field_names() {
        let json = serde_json::to_value(DriverSignals::browser()).expect("signals serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "actions": [],
                "screenshot": false,
                "pdf": false,
                "prior_block_signals": 2,
                "profile_required": false,
                "url_type": "Default",
            })
        );
    }

    #[test]
    fn default_signals_would_not_select_the_browser() {
        // Mirrors `plan_from_signals`: these values produce
        // `static_fetch("no signals")`, which is why the escalation has
        // to raise `prior_block_signals` rather than only send `render`.
        let json = serde_json::to_value(DriverSignals::default()).expect("signals serialize");
        assert_eq!(json["prior_block_signals"], 0);
        assert_eq!(json["actions"], serde_json::json!([]));
        assert_eq!(json["profile_required"], false);
    }

    fn ok_body(markdown_ref: Option<(&str, u64)>) -> serde_json::Value {
        match markdown_ref {
            Some((id, bytes)) => serde_json::json!({
                "data": {"status": 200, "formats": {"markdown": {"artifact_id": id, "bytes": bytes}}}
            }),
            None => serde_json::json!({"data": {"status": 200}}),
        }
    }

    fn test_client(base_url: String) -> Client {
        Client::new(Config {
            base_url,
            token: "test-token".to_owned(),
            timeout: Duration::from_secs(5),
        })
    }

    /// The fast path: a page that yields text on the first fetch must
    /// cost exactly one static request and never pay for a browser.
    #[tokio::test]
    async fn readable_scrape_does_not_escalate_when_text_is_present() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(
                serde_json::json!({"url": "https://example.com", "zdr": false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"status": 200, "formats": {"markdown": "# real content"}}
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://example.com", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# real content");
    }

    /// An empty body escalates ONCE, and the retry carries both the
    /// browser signals and the render hints.
    #[tokio::test]
    async fn readable_scrape_escalates_once_on_empty_text() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(
                serde_json::json!({"url": "https://ssb.no", "zdr": false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(None)))
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://ssb.no",
                "render": {"waitForSelector": "body", "waitForTimeoutMs": 2_500},
                "signals": {
                    "actions": [],
                    "screenshot": false,
                    "pdf": false,
                    "prior_block_signals": 2,
                    "profile_required": false,
                    "url_type": "Default",
                },
                "zdr": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"status": 200, "formats": {"markdown": "# rendered content"}}
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# rendered content");
    }

    /// Both attempts empty: exactly two requests (never a third), and
    /// the honest empty result is preserved for the caller to report.
    #[tokio::test]
    async fn readable_scrape_never_escalates_twice() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(None)))
            .expect(2)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert!(
            result.text.is_empty(),
            "empty must stay empty, not invented"
        );
    }

    /// An error is not an empty page: rendering cannot fix a 403, so it
    /// must surface immediately instead of costing a browser fetch.
    #[tokio::test]
    async fn readable_scrape_does_not_escalate_an_error() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {"code": "FORBIDDEN", "message": "blocked"}
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let error = test_client(quarry.uri())
            .scrape_readable("https://blocked.example", "org-a", false)
            .await
            .expect_err("error must propagate");
        assert!(matches!(error, QuarryError::Typed { status: 403, .. }));
    }

    /// If the escalated retry itself fails, the first (empty) result is
    /// returned — the caller reports "unread", not a hard error.
    #[tokio::test]
    async fn readable_scrape_keeps_first_result_when_the_retry_fails() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(
                serde_json::json!({"url": "https://ssb.no", "zdr": false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(None)))
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://ssb.no",
                "render": {"waitForSelector": "body", "waitForTimeoutMs": 2_500},
                "signals": {
                    "actions": [],
                    "screenshot": false,
                    "pdf": false,
                    "prior_block_signals": 2,
                    "profile_required": false,
                    "url_type": "Default",
                },
                "zdr": false,
            })))
            .respond_with(ResponseTemplate::new(504))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", false)
            .await
            .expect("first result is kept");
        assert!(result.text.is_empty());
    }

    /// The real defect: Quarry returns page text as a `FormatRef`, so
    /// the client has to resolve the artifact to see any content.
    #[tokio::test]
    async fn artifact_referenced_markdown_is_resolved_into_text() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ok_body(Some(("art_01ABC", 42)))),
            )
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/artifacts/art_01ABC"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# Oslo\n\nBefolkning"))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://www.ssb.no/kommunefakta/oslo", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# Oslo\n\nBefolkning");
        assert_eq!(result.markdown, "# Oslo\n\nBefolkning");
    }

    /// An unresolvable reference must not become fabricated content, and
    /// must not stop the escalation from being attempted.
    #[tokio::test]
    async fn unresolvable_artifact_reference_leaves_the_page_unread() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ok_body(Some(("art_01DENIED", 99)))),
            )
            .expect(2)
            .mount(&quarry)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/artifacts/art_01DENIED"))
            .respond_with(ResponseTemplate::new(403))
            .expect(2)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", false)
            .await
            .expect("scrape still succeeds");
        assert!(result.text.is_empty());
    }

    /// Zero-retention turns must never pull persisted bytes back out of
    /// the artifact store.
    #[tokio::test]
    async fn zdr_scrape_does_not_resolve_artifacts() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ok_body(Some(("art_01ZDR", 10)))),
            )
            .mount(&quarry)
            .await;
        // No /v1/artifacts mock: any GET would fail the test as an
        // unmatched request.

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", true)
            .await
            .expect("scrape succeeds");
        assert!(result.text.is_empty());
    }

    #[test]
    fn artifact_ref_reads_the_format_ref_shape() {
        let data = serde_json::json!({
            "formats": {"markdown": {"artifact_id": "art_01X", "bytes": 11_837}}
        });
        assert_eq!(
            artifact_ref(&data, "markdown"),
            Some(ArtifactRef {
                artifact_id: "art_01X".to_owned(),
                bytes: 11_837
            })
        );
        // An inline string is not a reference (tolerated, not resolved).
        let inline = serde_json::json!({"formats": {"markdown": "# hi"}});
        assert_eq!(artifact_ref(&inline, "markdown"), None);
        assert_eq!(artifact_ref(&data, "html"), None);
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

    /// An empty `/v1/search` response in the edge's real shape — top-level
    /// `results` beside `query`/`provider`/`count`, no `data` wrapper. The
    /// wire tests below assert request bodies, but they should still be
    /// answered with something an edge could actually have sent.
    fn search_ok() -> serde_json::Value {
        serde_json::json!({
            "query": "",
            "provider": "smart_router",
            "results": [],
            "count": 0,
        })
    }

    /// The narrowing search options must be invisible until someone sets one.
    /// `body_json` is an exact match, so a stray key — or an option
    /// serialised as `null` instead of being skipped — fails here rather
    /// than reaching an edge build that predates these fields.
    /// `allow_paid_providers` is the one key that is always present.
    #[tokio::test]
    async fn search_without_options_adds_no_narrowing_keys() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "kpi norge",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search("kpi norge", 5, "research", "org-a", false)
            .await
            .expect("search succeeds");
    }

    /// The same must hold for the options-taking entry point when the
    /// options are left at their defaults — otherwise every call site that
    /// migrates to it silently changes its wire shape.
    #[tokio::test]
    async fn default_options_add_no_narrowing_keys() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "kpi norge",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search_with_options(
                "kpi norge",
                5,
                "research",
                "org-a",
                false,
                &SearchOptions::default(),
            )
            .await
            .expect("search succeeds");
    }

    /// Pins the exact field names `quarry-edge`'s `SearchRequest` declares.
    /// It deserialises without `deny_unknown_fields`, so a renamed or
    /// misspelled key here would be discarded upstream without an error —
    /// this assertion is the only place that mismatch can be caught.
    #[tokio::test]
    async fn set_options_serialize_with_quarry_edges_field_names() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "konsumprisindeksen",
                "limit": 5,
                "intent": "fresh",
                "zdr": false,
                "allow_paid_providers": false,
                "language": "nb",
                "country": "NO",
                "topic": "news",
                "time_range": "week",
                "include_domains": ["ssb.no", "regjeringen.no"],
                "exclude_domains": ["pinterest.com"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search_with_options(
                "konsumprisindeksen",
                5,
                "fresh",
                "org-a",
                false,
                &SearchOptions {
                    language: Some("nb".to_owned()),
                    country: Some("NO".to_owned()),
                    topic: Some("news".to_owned()),
                    time_range: Some("week".to_owned()),
                    include_domains: vec!["ssb.no".to_owned(), "regjeringen.no".to_owned()],
                    exclude_domains: vec!["pinterest.com".to_owned()],
                    allow_paid_providers: false,
                },
            )
            .await
            .expect("search succeeds");
    }

    /// Blank options come from model-authored tool arguments and must drop
    /// out entirely: `Some("")` is a different edge cache key from `None`,
    /// so a blank would fragment the cache without changing a result.
    #[tokio::test]
    async fn blank_options_are_omitted_rather_than_sent_empty() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "kpi norge",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search_with_options(
                "kpi norge",
                5,
                "research",
                "org-a",
                false,
                &SearchOptions {
                    language: Some("  ".to_owned()),
                    country: Some(String::new()),
                    topic: None,
                    time_range: Some(String::new()),
                    include_domains: vec![String::new(), "   ".to_owned()],
                    exclude_domains: Vec::new(),
                    allow_paid_providers: false,
                },
            )
            .await
            .expect("search succeeds");
    }

    /// Whitespace around a real value is trimmed rather than forwarded: the
    /// edge matches `time_range` against a closed set and would drop
    /// `" week"` as unrecognised, silently losing the recency window.
    #[tokio::test]
    async fn option_values_are_trimmed() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "statsbudsjettet",
                "limit": 5,
                "zdr": false,
                "allow_paid_providers": false,
                "time_range": "week",
                "include_domains": ["ssb.no"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search_with_options(
                "statsbudsjettet",
                5,
                "",
                "org-a",
                false,
                &SearchOptions {
                    time_range: Some(" week ".to_owned()),
                    include_domains: vec![" ssb.no ".to_owned(), "  ".to_owned()],
                    ..SearchOptions::default()
                },
            )
            .await
            .expect("search succeeds");
    }

    /// The live contract: `quarry-edge` returns `Json(SearchResponse)`, so
    /// `results` sits at the top level next to `query`/`provider`/`count`.
    /// This is the shape every production search response actually has.
    /// The grant must be on the wire even when it is denied. An absent field
    /// means "no paid providers" on the edge, so a skipped `false` would still
    /// be *obeyed* — but it would be indistinguishable from a client too old to
    /// know about tiering, which is not a thing an egress decision may be
    /// ambiguous about after the fact.
    #[tokio::test]
    async fn the_paid_provider_grant_is_always_on_the_wire() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "kpi norge",
                "limit": 5,
                "zdr": false,
                "allow_paid_providers": true,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .search_with_options(
                "kpi norge",
                5,
                "",
                "org-a",
                false,
                &SearchOptions {
                    allow_paid_providers: true,
                    ..SearchOptions::default()
                },
            )
            .await
            .expect("search succeeds");
    }

    #[test]
    fn extract_search_results_reads_the_edges_top_level_results() {
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

    /// BFF compatibility only — NOT a shape any edge produces.
    ///
    /// The `{data: ...}` wrapper is the Frontend Plane BFF's, and
    /// model-gateway does not reach search through the BFF. This test used to
    /// be read as pinning the edge's contract, which is how the client came to
    /// try `data.results` first and run the entire live path on its fallback.
    /// It is kept so a payload relayed through the BFF still parses, and for
    /// no stronger claim than that.
    #[test]
    fn extract_search_results_still_accepts_a_bff_wrapped_response() {
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

    /// A response carrying BOTH shapes must be read from the top level. Every
    /// other test here would pass under either ordering — a body with only
    /// top-level `results` parses fine as a fallback — so this is the one that
    /// actually pins which branch is primary, and the one that fails if the
    /// `data`-first order is ever restored.
    #[test]
    fn top_level_results_win_over_a_wrapped_copy() {
        let payload = serde_json::json!({
            "query": "kpi",
            "provider": "smart_router",
            "results": [{"url": "https://edge.example/", "title": "edge"}],
            "count": 1,
            "data": {"results": [{"url": "https://bff.example/", "title": "bff"}]}
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].url, "https://edge.example/",
            "the edge's own top-level results must be preferred over a wrapper"
        );
    }

    /// Cross-engine agreement is the signal, so the engine list has to survive
    /// the projection rather than being left in `raw` for someone to rediscover.
    #[test]
    fn engines_survive_the_projection() {
        let payload = serde_json::json!({
            "results": [{
                "url": "https://www.ssb.no/kpi",
                "title": "KPI",
                "engines": ["brave", "duckduckgo", "  ", "google", ""]
            }]
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(
            results[0].engines,
            vec![
                "brave".to_owned(),
                "duckduckgo".to_owned(),
                "google".to_owned()
            ],
            "blank entries are not votes and must not inflate the agreement count"
        );
    }

    /// Until the Ingestion Plane ships the field, every response omits it. That
    /// has to project as "no engines reported", not fail the whole row.
    #[test]
    fn an_absent_engine_list_projects_as_empty() {
        let payload = serde_json::json!({
            "results": [{"url": "https://a.no/", "title": "A", "rank": 1}]
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert!(results[0].engines.is_empty());
    }

    /// Quarry's `rank`, reranker `score` and `highlights` were all reaching this
    /// client and all being thrown away here — `rank` and `highlights` were never
    /// projected at all, and `score` was flattened to `0.0` when absent. The
    /// relevance gate is built on these three, so this test is the contract that
    /// they survive the projection.
    #[test]
    fn search_projection_keeps_rank_reranker_score_and_highlights() {
        let payload = serde_json::json!({
            "data": {"results": [{
                "url": "https://www.ssb.no/kpi",
                "title": "Konsumprisindeksen",
                "snippet": "KPI for Norge.",
                "provider": "brave",
                "rank": 3,
                "score": 0.91,
                "highlights": ["KPI steg 2,4 prosent", "   ", ""]
            }]}
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(results[0].rank, 3);
        assert_eq!(results[0].relevance, Some(0.91));
        assert!((results[0].score - 0.91).abs() < f32::EPSILON);
        assert_eq!(
            results[0].highlights,
            vec!["KPI steg 2,4 prosent".to_owned()],
            "blank highlights are dropped rather than shown as empty evidence"
        );
    }

    /// An absent `score` must project as `None`, not as `0.0`.
    ///
    /// Quarry omits the field entirely whenever it did not rerank — reranking
    /// disabled, the Model Plane call failed, or the hit fell outside the
    /// reranked head — and the whole tail of every reranked response is in that
    /// state. Reading absent as "scored zero" would make the relevance gate
    /// filter an unreranked deployment down to nothing.
    #[test]
    fn an_absent_reranker_score_projects_as_none_not_zero() {
        let payload = serde_json::json!({
            "results": [{"url": "https://a.no/", "title": "A", "rank": 1}]
        });

        let results: Vec<SearchResult> = extract_search_results(&payload)
            .iter()
            .map(project_search_result)
            .collect();

        assert_eq!(results[0].relevance, None);
        assert!(
            (results[0].score - 0.0).abs() < f32::EPSILON,
            "the compat mirror is still 0.0 for the proto field"
        );
        assert!(results[0].highlights.is_empty());
    }
}
