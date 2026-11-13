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
use crate::relevance::{self, ScoreKind};
use crate::relevance_calibration::{self, Calibration, CalibrationError};

const SCRAPE_SCOPES: &[&str] = &["scrape:read"];
const SEARCH_SCOPES: &[&str] = &["search:read"];

/// How long the browser driver may wait for client-rendered markup to
/// settle on the one escalated retry. Comfortably inside the 30s scrape
/// timeout so the wait can never be what times the call out.
const RENDER_SETTLE_MS: u32 = 2_500;

/// Advisory request attribution for the Quarry reads of one task (E4.6, 062a).
///
/// A caller runs its Quarry reads inside [`attribution::scope`]. Every edge request those reads send on
/// that task carries the scope's `x-request-id` and, when set, its `x-verevon-parent-run`, including the
/// retry after a 401. The value lives in a task-local, so it does not follow a `tokio::spawn`: a caller
/// must not wrap detached work. Both headers are attribution only. quarry-edge uses them for usage records
/// and logs, never for auth, scope, budget or idempotency, and it reads the parent run from a service
/// principal only. The switch `MODEL_GATEWAY_QUARRY_ATTRIBUTION` is off by default: with it off no scope is entered
/// and every read goes out as it was built.
pub mod attribution {
    use std::future::Future;

    /// The request-id header quarry-edge adopts (`QUARRY_EDGE__REQUEST_ID_ADOPT`).
    const REQUEST_ID_HEADER: &str = "x-request-id";
    /// The advisory parent-run header quarry-edge reads from service principals only.
    const PARENT_RUN_HEADER: &str = "x-verevon-parent-run";
    /// The longest value, in bytes, that either header carries.
    const MAX_VALUE_LEN: usize = 128;

    tokio::task_local! {
        /// The attribution of the Quarry reads running on this task.
        static CURRENT: Attribution;
    }

    /// The attribution stamped on the Quarry requests made inside one [`scope`].
    #[derive(Clone)]
    pub struct Attribution {
        /// The request id, sent as `x-request-id` when it is a valid value.
        pub request_id: Option<String>,
        /// The run the reads belong to, sent as `x-verevon-parent-run` when it is a valid value. A caller
        /// under ZDR leaves it `None`, so no run id leaves the Model Plane.
        pub parent_run: Option<String>,
    }

    /// The switch that turns attribution on. Off unless its value is the literal `true`.
    pub const ENABLED_ENV: &str = "MODEL_GATEWAY_QUARRY_ATTRIBUTION";

    /// Whether a raw switch value turns attribution on: only the literal `true`.
    #[must_use]
    pub fn parse_enabled(raw: Option<&str>) -> bool {
        raw == Some("true")
    }

    /// Whether attribution is on in this process. Read on every scope, so a restart is not needed to turn it off.
    #[must_use]
    pub fn enabled() -> bool {
        parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
    }

    /// Runs `fut` with `attribution` in scope, so every Quarry read it makes on this task is stamped. With the switch
    /// off the future runs unscoped, so no attribution header is sent.
    pub async fn scope<F: Future>(attribution: Attribution, fut: F) -> F::Output {
        scope_when(enabled(), attribution, fut).await
    }

    /// [`scope`] with the switch given, so both settings can be tested without touching the process environment.
    pub(crate) async fn scope_when<F: Future>(
        on: bool,
        attribution: Attribution,
        fut: F,
    ) -> F::Output {
        if on {
            CURRENT.scope(attribution, fut).await
        } else {
            fut.await
        }
    }

    /// Stamps the scoped attribution on a Quarry request. Outside a scope the request goes out as it was
    /// built. A value outside 1 to 128 bytes of `[A-Za-z0-9._:-]` is left off, since quarry-edge would not
    /// adopt it either.
    pub(crate) fn apply(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let Ok(current) = CURRENT.try_with(Clone::clone) else {
            return req;
        };
        let req = match current.request_id.as_deref().filter(|id| valid_value(id)) {
            Some(id) => req.header(REQUEST_ID_HEADER, id),
            None => req,
        };
        match current.parent_run.as_deref().filter(|run| valid_value(run)) {
            Some(run) => req.header(PARENT_RUN_HEADER, run),
            None => req,
        }
    }

    /// Whether a value may go on the wire: 1 to 128 bytes from `[A-Za-z0-9._:-]`, the set quarry-edge adopts.
    fn valid_value(value: &str) -> bool {
        (1..=MAX_VALUE_LEN).contains(&value.len())
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
    }
}

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

impl QuarryError {
    /// A class of this error that is safe to log: a typed envelope's code, or a
    /// fixed word. Never the display text, which can carry the page URL.
    pub(crate) fn log_class(&self) -> String {
        match self {
            Self::Unavailable => "unavailable".to_string(),
            Self::Typed { code, .. } => code.clone(),
            Self::Transport(_) => "transport".to_string(),
            Self::EmptyEnvelope => "empty_envelope".to_string(),
            Self::Decode(_) => "decode".to_string(),
            Self::Authentication(_) => "authentication".to_string(),
        }
    }
}

/// What kind of host a caller URL names, for log lines: the URL itself is the
/// caller's data and stays out of the logs.
fn host_class(url: &str) -> &'static str {
    let parsed = reqwest::Url::parse(url).ok();
    match parsed.as_ref().and_then(|u| u.host_str()) {
        Some(host) if host.trim_matches(['[', ']']).parse::<std::net::IpAddr>().is_ok() => {
            "ip_literal"
        }
        Some(_) => "hostname",
        None => "invalid",
    }
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
    /// Deprecated (WS3.I). Under `QUARRY_BLOCK_ESCALATION=legacy` `>= 2`
    /// selects the browser driver and `== 1` the TLS-profile driver; under
    /// `honest`, the edge default, the field is accepted and ignored. A caller
    /// that wants a browser render for a content reason must not use it:
    /// use [`DriverSignals::browser`], which sets `profile_required`.
    pub prior_block_signals: u8,
    /// Selects the browser driver in every escalation mode.
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
    /// Uses `profile_required = true`, which `plan_from_signals` in
    /// `quarry-runtime/src/driver_plan.rs` turns into the browser driver in
    /// every escalation mode. A static fetch that yields no readable text is a
    /// content condition (a client-rendered page), not an HTTP block, so this
    /// asks for a render explicitly. It used to send `prior_block_signals = 2`,
    /// which the edge ignores by default since WS3.I (`honest`), so the retry
    /// would have repeated the same static fetch and the page cache would no
    /// longer be bypassed. Deliberately does not set `screenshot` / `pdf` /
    /// `actions`, which would also force a browser but bill an artifact we do
    /// not want.
    #[must_use]
    pub fn browser() -> Self {
        Self {
            profile_required: true,
            ..Self::default()
        }
    }
}

/// Per-call scrape knobs. Added as a struct rather than more positional
/// parameters so new Quarry request fields don't churn every call site.
#[derive(Debug, Clone)]
pub struct ScrapeOptions {
    pub render: Option<RenderHints>,
    pub signals: Option<DriverSignals>,
    pub prefer_http3: bool,
    pub zdr: bool,
    /// Output formats to request, in the edge's own spelling. Always sent
    /// (unless empty) so the edge never has to guess: `["markdown"]` makes it
    /// return the page text inline in `data.markdown`, which saves the second
    /// call to `/v1/artifacts/{id}`, and is the only way text reaches us under
    /// ZDR.
    pub formats: Vec<&'static str>,
}

impl Default for ScrapeOptions {
    fn default() -> Self {
        Self {
            render: None,
            signals: None,
            prefer_http3: false,
            zdr: false,
            formats: vec!["markdown"],
        }
    }
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
            zdr,
            ..Self::default()
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
    /// Quarry's relevance judgement in `[0,1]`, or `None` when this hit was not
    /// judged.
    ///
    /// Quarry reorders its results with the Data Plane's cross-encoder and, only
    /// for a caller that asked for `cross_encoder.v1`, names that kind on the
    /// answer. The raw cross-encoder score is not a relevance probability, so it
    /// is read here only through its calibration table
    /// ([`crate::relevance::provider_score_for_gate`]); without one it is absent. A
    /// legacy answer's `score` is read as it always was. The field is omitted
    /// entirely otherwise: reranking is off for the deployment, the rerank did not
    /// apply (skipped, degraded or shadow), or the hit fell outside the reranked
    /// head. Absent is therefore "not judged", which is a different fact from "judged
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
    /// The kind of the score on this hit, when the score was read through a calibration
    /// (`cross_encoder.v1`). `None` for a legacy score and for an unjudged hit.
    pub score_kind: Option<ScoreKind>,
    pub raw: Value,
}

impl SearchResult {
    /// The score a consumer of this hit reads, or `None` when the hit was not judged.
    ///
    /// A legacy score of zero means "not judged", the rule the gate has always had, because the
    /// flattened `score` is 0.0 whenever Quarry sent none. A calibrated cross-encoder score of zero
    /// is a judgement that the hit is irrelevant, so it is kept.
    #[must_use]
    pub fn wire_score(&self) -> Option<f32> {
        self.relevance
            .filter(|score| *score > 0.0 || self.score_kind.is_some())
    }
}

/// What the rerank step did for one `/v1/search` answer, as Quarry named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RerankStatus {
    /// The scores ordered the leading results.
    Applied,
    /// The reranker was not called, and the reason says why (`zdr`, `disabled`, ...).
    Skipped,
    /// The reranker was called and failed, so the results keep their order.
    Degraded,
    /// The reranker was called and measured, but the results keep their order.
    Shadow,
}

impl RerankStatus {
    /// The word Quarry uses for this status on the wire.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Skipped => "skipped",
            Self::Degraded => "degraded",
            Self::Shadow => "shadow",
        }
    }
}

/// A rerank step's report: its status, and the reason Quarry gave for it when it gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankOutcome {
    pub status: RerankStatus,
    pub reason: Option<String>,
}

/// A `/v1/search` answer: the results, and what the rerank step did to them.
#[derive(Debug, Clone)]
pub struct SearchReply {
    pub results: Vec<SearchResult>,
    /// `None` when the edge sent no rerank report, or said the rerank was not configured. Either
    /// way the relevance gate has no rerank fact to report.
    pub rerank: Option<RerankOutcome>,
    /// The envelope's `provider`: the router or provider that answered, such as `smart_router`. A hit's own
    /// `source` names the provider that served it, so this is read only for a hit that names none, and for a
    /// search that returned no hits (EP-9 W1). Empty when the edge sent none.
    pub provider: String,
}

/// One `web_search` hit as the tool loop and deep research read it back.
#[must_use]
pub fn web_search_item(result: &SearchResult, rerank: Option<&RerankOutcome>) -> Value {
    let mut item = serde_json::json!({
        "url": result.url, "title": result.title, "snippet": result.snippet
    });
    if let Some(score) = result.wire_score() {
        item["score"] = serde_json::json!(score);
    }
    if !result.highlights.is_empty() {
        item["highlights"] = serde_json::json!(result.highlights);
    }
    if result.rank > 0 {
        item["rank"] = serde_json::json!(result.rank);
    }
    if !result.engines.is_empty() {
        item["engines"] = serde_json::json!(result.engines);
    }
    if let Some(outcome) = rerank {
        item["rerank_status"] = serde_json::json!(outcome.status.label());
        if let Some(reason) = &outcome.reason {
            item["rerank_reason"] = serde_json::json!(reason);
        }
    }
    item
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

/// The statuses the Quarry edge treats as an origin block
/// (`quarry_runtime::fingerprint_rotation::is_block_status`). Kept in step by
/// hand: this crate does not depend on Quarry's workspace.
fn is_block_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 407 | 429 | 503)
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
    /// The calibration that lets this gateway ask Quarry for cross-encoder scores. `None`, the
    /// default, asks for none: Quarry then returns the order alone and no score reaches the gate.
    cross_encoder: Option<Calibration>,
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
            cross_encoder: None,
        }
    }

    /// Apply the boot-time cross-encoder decision (`VEREVON_RELEVANCE_CROSS_ENCODER_SCORES`).
    ///
    /// With the switch off this is a no-op, so the client asks Quarry for the order alone, as it
    /// always has. With the switch on, the shipped calibration must be fitted; otherwise the boot is
    /// refused.
    ///
    /// # Errors
    ///
    /// Refuses a switch value that is neither on nor off, and a switch that is on against an
    /// unfitted or invalid calibration artefact.
    pub fn with_cross_encoder_scores_from_env(mut self) -> Result<Self, CalibrationError> {
        self.cross_encoder = relevance_calibration::from_environment()?;
        Ok(self)
    }

    /// Ask for cross-encoder scores against a given calibration. Test-only: production reaches it only
    /// through [`Self::with_cross_encoder_scores_from_env`], which checks the shipped artefact.
    #[cfg(test)]
    pub(crate) fn with_cross_encoder_scores(mut self, calibration: Calibration) -> Self {
        self.cross_encoder = Some(calibration);
        self
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
            cross_encoder: None,
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
                prefer_http3,
                zdr,
                ..ScrapeOptions::default()
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
    /// Escalation happens **only** for an empty HTML body that answered
    /// with a success status — never for an error (a 403/timeout is not
    /// fixed by rendering, and retrying it would double the cost of every
    /// genuinely broken URL, a typed `UNSUPPORTED_MEDIA` included), never
    /// for a page the origin answered with a block status (401, 403, 407,
    /// 429, 503: a direct-egress block is returned by the edge as a normal
    /// envelope whose `data.status` carries it, not as an error, and the
    /// edge's own block escalation refuses to change identity on it, so this
    /// retry must not either; the browser retry is for a page that
    /// withheld its content, not for one that refused the request), never
    /// for a body the edge detected
    /// as something other than HTML (a browser cannot read a PNG any
    /// better than the static fetch did) and never more than once. If the
    /// rendered retry is also empty (or itself fails) the first result is
    /// returned unchanged, so the caller still reports the page as unread
    /// rather than inventing content.
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
        if let Some(kind) = content_kind(&first.raw).filter(|kind| *kind != "html") {
            tracing::debug!(
                host_class = host_class(url),
                content_kind = kind,
                "quarry scrape returned no readable text for a non-html body; not escalating"
            );
            return Ok(first);
        }

        // A block status is not a page that withheld its content: the origin
        // refused the request. Rendering it in a browser would be a status
        // driven change of identity (ADR-5), so the refusal is returned as it
        // is and the caller reports the page as unread. The scrape output
        // carries no challenge marker for a 200 interstitial (the edge
        // classifies and counts it, or refuses it with CHALLENGE_DETECTED,
        // which arrives above as an error), so the status is the only signal
        // this client has.
        if is_block_status(first.status) {
            tracing::debug!(
                host_class = host_class(url),
                status = first.status,
                "quarry scrape answered with a block status and no readable text; not escalating"
            );
            return Ok(first);
        }

        tracing::debug!(
            host_class = host_class(url),
            status = first.status,
            "quarry scrape returned no readable text; escalating once to the browser driver"
        );
        match self
            .scrape_with_options(url, org_id, &ScrapeOptions::browser_escalation(zdr))
            .await
        {
            Ok(rendered) if !rendered.text.trim().is_empty() => {
                tracing::debug!(
                    host_class = host_class(url),
                    chars = rendered.text.chars().count(),
                    "rendered retry recovered readable text"
                );
                Ok(rendered)
            }
            Ok(_) => {
                tracing::debug!(host_class = host_class(url), "rendered retry also returned no readable text");
                Ok(first)
            }
            Err(error) => {
                tracing::debug!(
                    host_class = host_class(url),
                    error_class = %error.log_class(),
                    "rendered retry failed; keeping the empty result"
                );
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
            /// Always sent while non-empty, so the edge does not have to
            /// pick a default for us (under ZDR its default is inline
            /// markdown, otherwise an artifact reference).
            #[serde(skip_serializing_if = "<[_]>::is_empty")]
            formats: &'a [&'static str],
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
            formats: &options.formats,
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
            let resp = attribution::apply(req).send().await?;
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

        // `project` has already taken the inline `data.markdown` the edge
        // returns for `formats: ["markdown"]`, so this is the fallback for a
        // response that did not inline it: an edge that predates the field, or
        // one that left it out as over the inline cap (`omitted`). There
        // `OutputFormats.markdown` is an `Option<FormatRef>`
        // — `{artifact_id, bytes}` — and resolving it gets the bytes the fetch
        // already produced, at the price of a second call.
        //
        // Skipped under ZDR: on a zero-retention turn Quarry stores nothing
        // to read back, and pulling persisted bytes would defeat the
        // guarantee even if it did.
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
                    host_class = host_class(url),
                    artifact_id = %reference.artifact_id,
                    "quarry markdown artifact resolved to an empty body"
                );
                None
            }
            Err(error) => {
                tracing::warn!(
                    host_class = host_class(url),
                    artifact_id = %reference.artifact_id,
                    bytes = reference.bytes,
                    error_class = %error.log_class(),
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
            let resp = attribution::apply(req).send().await?;
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

    /// `GET /v1/agent/runs/{run_id}/procedure-draft` on the edge (Teach mode, F-t3-081), as the
    /// person who drove the run. `bearer` is that person's own `aud=quarry` credential, which the
    /// caller verified; it is never this gateway's service principal, because the edge checks the
    /// run's owner and the owner is a person. Returns the answer's `data` object: `run_id`,
    /// `steps`, `params` and `truncated`.
    ///
    /// A typed refusal keeps its status. Its code is the edge's `details.reason` when it names one
    /// (`zdr_not_supported`, `teach_mode_disabled`), and the envelope's code otherwise.
    ///
    /// # Errors
    ///
    /// Returns a [`QuarryError`] if the edge is unavailable, answers a refusal, or answers a body
    /// that is not the envelope it names.
    pub async fn procedure_draft(&self, run_id: &str, bearer: &str) -> Result<Value, QuarryError> {
        if !self.available() {
            return Err(QuarryError::Unavailable);
        }
        let endpoint = format!("{}/v1/agent/runs/{run_id}/procedure-draft", self.base_url);
        let resp = self.http.get(&endpoint).bearer_auth(bearer).send().await?;
        let status = resp.status().as_u16();
        let raw = resp.text().await?;
        if status >= 400 {
            let parsed: Option<Value> = serde_json::from_str(&raw).ok();
            let error = parsed.as_ref().and_then(|value| value.get("error"));
            let reason = error
                .and_then(|error| error.get("details"))
                .and_then(|details| details.get("reason"))
                .and_then(Value::as_str);
            let code = error
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str);
            let message = error
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str);
            return Err(QuarryError::Typed {
                code: reason
                    .or(code)
                    .map_or_else(|| format!("HTTP_{status}"), str::to_owned),
                message: message.map_or_else(|| truncate(&raw, 200), str::to_owned),
                status,
            });
        }
        let env: Value = serde_json::from_str(&raw)?;
        env.get("data")
            .filter(|data| data.is_object())
            .cloned()
            .ok_or(QuarryError::EmptyEnvelope)
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
        self.search_reply(query, limit, intent, org_id, zdr, options)
            .await
            .map(|reply| reply.results)
    }

    /// Call `/v1/search` and return the results with the rerank report Quarry sent.
    ///
    /// The request asks for cross-encoder scores only when this client holds a calibration, so a
    /// gateway without one sends the legacy body and receives the order alone.
    ///
    /// # Errors
    ///
    /// The same as [`Self::search_with_options`].
    pub async fn search_reply(
        &self,
        query: &str,
        limit: i32,
        intent: &str,
        org_id: &str,
        zdr: bool,
        options: &SearchOptions,
    ) -> Result<SearchReply, QuarryError> {
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
            /// The score kinds this gateway can read. Asked for only when it holds a calibration,
            /// and skipped when empty, so the legacy body is unchanged.
            #[serde(skip_serializing_if = "Vec::is_empty")]
            score_kinds: Vec<&'static str>,
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
            score_kinds: if self.cross_encoder.is_some() {
                vec![ScoreKind::CrossEncoderV1.as_str()]
            } else {
                Vec::new()
            },
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
            let resp = attribution::apply(req).send().await?;
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
        Ok(reply_from_payload(&payload, self.cross_encoder.as_ref()))
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
        score_kind: None,
        raw: Value::Object(obj),
    }
}

/// Read a whole `/v1/search` answer: its results, read through the score kind Quarry named, and its
/// rerank report.
///
/// A score is read only when the rerank that produced it applied, or when the answer carries no rerank
/// report at all (a legacy edge). A skipped, degraded or shadow rerank supplies no usable score, even if
/// a misbehaving edge sent one, so the gate never judges on a score the rerank did not stand behind.
fn reply_from_payload(payload: &Value, calibration: Option<&Calibration>) -> SearchReply {
    let kind = payload.get("score_kind").and_then(Value::as_str);
    let rerank = rerank_from_payload(payload.get("rerank"));
    // A report this gateway does not name is neither an applied rerank nor a legacy answer, so no score
    // is read from it. Only an answer that carries no report at all is read as a legacy answer.
    let scores_trusted = match (rerank.as_ref(), payload.get("rerank")) {
        (Some(outcome), _) => outcome.status == RerankStatus::Applied,
        (None, report) => report.is_none(),
    };
    let results = extract_search_results(payload)
        .iter()
        .map(project_search_result)
        .map(|mut result| {
            let judged = if scores_trusted {
                relevance::provider_score_for_gate(kind, result.relevance, calibration)
            } else {
                None
            };
            result.relevance = judged;
            result.score = judged.unwrap_or(0.0);
            result.score_kind = kind.and_then(ScoreKind::parse).filter(|_| judged.is_some());
            result
        })
        .collect();
    // Read whole rather than per hit: the envelope names the answering router, which the disclosure line needs
    // for a hit that names no provider of its own.
    let provider = payload
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    SearchReply {
        results,
        rerank,
        provider,
    }
}

/// The rerank report Quarry sent, or `None` for an absent report or the `rerank-not-configured`
/// status, and for any status this gateway does not name.
fn rerank_from_payload(report: Option<&Value>) -> Option<RerankOutcome> {
    let report = report?.as_object()?;
    let status = match report.get("status")?.as_str()? {
        "applied" => RerankStatus::Applied,
        "skipped" => RerankStatus::Skipped,
        "degraded" => RerankStatus::Degraded,
        "shadow" => RerankStatus::Shadow,
        _ => return None,
    };
    let reason = report
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(RerankOutcome { status, reason })
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

    // The edge inlines the page text as top-level `markdown` when asked for
    // `formats: ["markdown"]` (and by default under ZDR). An edge that
    // predates that field put a string at `formats.markdown` instead; keep
    // reading it. A `formats.markdown` that is a `FormatRef` object is not a
    // string and stays for `artifact_ref`.
    let formats = obj.get("formats").and_then(Value::as_object);
    let inline_markdown = obj
        .get("markdown")
        .and_then(Value::as_str)
        .filter(|md| !md.is_empty());
    let legacy_markdown = formats
        .and_then(|f| f.get("markdown"))
        .and_then(Value::as_str);
    let markdown = inline_markdown
        .or(legacy_markdown)
        .unwrap_or("")
        .to_string();
    let plain_text = formats
        .and_then(|f| f.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // Fall back so `.text` is always populated.
    let text = if plain_text.is_empty() {
        markdown.clone()
    } else {
        plain_text.to_string()
    };

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

/// What the edge detected the body to be (`html`, `pdf`, `image`, ...), from
/// `data.metadata.content_kind`. Absent when the edge has content routing off
/// or predates it.
fn content_kind(data: &Value) -> Option<&str> {
    data.get("metadata")?
        .get("content_kind")?
        .as_str()
        .filter(|kind| !kind.is_empty())
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

    #[test]
    fn log_lines_carry_a_host_class_and_an_error_class_never_the_url() {
        assert_eq!(host_class("https://shop.example/cat?q=secret"), "hostname");
        assert_eq!(host_class("http://10.0.0.7:8080/x"), "ip_literal");
        assert_eq!(host_class("http://[::1]/x"), "ip_literal");
        assert_eq!(host_class("not a url"), "invalid");
        let typed = QuarryError::Typed {
            code: "UPSTREAM_BLOCKED".to_owned(),
            message: "blocked https://shop.example/cat?q=secret".to_owned(),
            status: 502,
        };
        assert_eq!(typed.log_class(), "UPSTREAM_BLOCKED");
        assert!(!typed.log_class().contains("shop.example"));
        assert_eq!(QuarryError::EmptyEnvelope.log_class(), "empty_envelope");
        assert_eq!(QuarryError::Unavailable.log_class(), "unavailable");
    }

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
                "zdr": true,
                "formats": ["markdown"]
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
            cross_encoder: None,
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
                "prior_block_signals": 0,
                "profile_required": true,
                "url_type": "Default",
            })
        );
    }

    #[test]
    fn default_signals_would_not_select_the_browser() {
        // Mirrors `plan_from_signals`: these values produce
        // `static_fetch("no signals")`, which is why the escalation has
        // to set `profile_required` rather than only send `render`.
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
                serde_json::json!({"url": "https://example.com", "zdr": false, "formats": ["markdown"]}),
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
                serde_json::json!({"url": "https://ssb.no", "zdr": false, "formats": ["markdown"]}),
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
                    "prior_block_signals": 0,
                    "profile_required": true,
                    "url_type": "Default",
                },
                "zdr": false,
                "formats": ["markdown"],
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

    /// A block status that the edge returned as a normal envelope (a
    /// direct-egress 403, 429 or 503 is `{data:{status:N}}`, not an error)
    /// is not a page that withheld its content: it must not be re-fetched
    /// through the browser. Exactly one request, and the browser-escalation
    /// body is mounted with `expect(0)`.
    #[tokio::test]
    async fn readable_scrape_does_not_escalate_a_block_status_page() {
        for status in [401_u16, 403, 407, 429, 503] {
            let quarry = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/scrape"))
                .and(body_json(
                    serde_json::json!({"url": "https://blocked.example", "zdr": false, "formats": ["markdown"]}),
                ))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"data": {"status": status}})),
                )
                .expect(1)
                .mount(&quarry)
                .await;
            Mock::given(method("POST"))
                .and(path("/v1/scrape"))
                .and(body_json(serde_json::json!({
                    "url": "https://blocked.example",
                    "render": {"waitForSelector": "body", "waitForTimeoutMs": 2_500},
                    "signals": {
                        "actions": [],
                        "screenshot": false,
                        "pdf": false,
                        "prior_block_signals": 0,
                        "profile_required": true,
                        "url_type": "Default",
                    },
                    "zdr": false,
                    "formats": ["markdown"],
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {"status": 200, "formats": {"markdown": "# rendered content"}}
                })))
                .expect(0)
                .mount(&quarry)
                .await;

            let result = test_client(quarry.uri())
                .scrape_readable("https://blocked.example", "org-a", false)
                .await
                .expect("a block status page is a result, not an error");
            assert_eq!(result.status, status);
            assert!(result.text.trim().is_empty(), "status {status}");
        }
    }

    /// If the escalated retry itself fails, the first (empty) result is
    /// returned — the caller reports "unread", not a hard error.
    #[tokio::test]
    async fn readable_scrape_keeps_first_result_when_the_retry_fails() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(
                serde_json::json!({"url": "https://ssb.no", "zdr": false, "formats": ["markdown"]}),
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
                    "prior_block_signals": 0,
                    "profile_required": true,
                    "url_type": "Default",
                },
                "zdr": false,
                "formats": ["markdown"],
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
        // wiremock answers an unmatched request with 404 and does not fail
        // the test, so the artifact read is mounted with a real body and
        // `expect(0)`: the count is checked when the server drops, so a
        // fetch under ZDR fails here (and the text assertion below too).
        Mock::given(method("GET"))
            .and(path("/v1/artifacts/art_01ZDR"))
            .respond_with(ResponseTemplate::new(200).set_body_string("persisted bytes"))
            .expect(0)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://ssb.no", "org-a", true)
            .await
            .expect("scrape succeeds");
        assert!(result.text.is_empty());
    }

    /// The point of asking for `formats: ["markdown"]`: the edge inlines the
    /// text, so a page read is one call. The `GET` mock expects zero hits even
    /// though the response also carries a `formats.markdown` reference.
    #[tokio::test]
    async fn inline_markdown_skips_the_artifact_call() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://example.com/a",
                "zdr": false,
                "formats": ["markdown"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "status": 200,
                    "markdown": "# Oslo\n\nBefolkning",
                    "metadata": {"title": "Oslo", "content_kind": "html"},
                    "formats": {"markdown": {"artifact_id": "art_01INLINE", "bytes": 18}},
                }
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/artifacts/art_01INLINE"))
            .respond_with(ResponseTemplate::new(200).set_body_string("stale"))
            .expect(0)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://example.com/a", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# Oslo\n\nBefolkning");
        assert_eq!(result.markdown, "# Oslo\n\nBefolkning");
        assert_eq!(result.title, "Oslo");
    }

    /// Under ZDR the edge stores nothing and returns the text inline. That
    /// must be read as content: one scrape, no browser retry, no artifact.
    #[tokio::test]
    async fn zdr_inline_markdown_does_not_escalate_to_the_browser() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://example.com/a",
                "zdr": true,
                "formats": ["markdown"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "status": 200,
                    "markdown": "# Zero retention",
                    "metadata": {"content_kind": "html"},
                }
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        // No artifact is referenced here and no artifact read is mounted;
        // an unmatched GET would get wiremock's 404, not a test failure.
        // The ZDR artifact guard is pinned by
        // `zdr_scrape_does_not_resolve_artifacts`.

        let result = test_client(quarry.uri())
            .scrape_readable("https://example.com/a", "org-a", true)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# Zero retention");
    }

    /// An edge that did not inline (older build, or the text was over the
    /// inline cap and listed in `omitted`) still works through the reference.
    #[tokio::test]
    async fn edge_without_inline_support_falls_back_to_the_artifact_call() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(serde_json::json!({
                "url": "https://example.com/big",
                "zdr": false,
                "formats": ["markdown"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "status": 200,
                    "omitted": ["markdown"],
                    "metadata": {"content_kind": "html"},
                    "formats": {"markdown": {"artifact_id": "art_01BIG", "bytes": 2_000_000}},
                }
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/artifacts/art_01BIG"))
            .respond_with(ResponseTemplate::new(200).set_body_string("# Big page"))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://example.com/big", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert_eq!(result.text, "# Big page");
    }

    /// A typed refusal (`UNSUPPORTED_MEDIA`, HTTP 415) is the edge saying it
    /// has no extractor for this body. A browser has none either, so it must
    /// surface at once as one call, not as a second scrape.
    #[tokio::test]
    async fn typed_unsupported_media_does_not_escalate() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(415).set_body_json(serde_json::json!({
                "error": {"code": "UNSUPPORTED_MEDIA", "message": "image/png"}
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let error = test_client(quarry.uri())
            .scrape_readable("https://example.com/logo.png", "org-a", false)
            .await
            .expect_err("a typed refusal must propagate");
        assert!(
            matches!(&error, QuarryError::Typed { code, status: 415, .. } if code == "UNSUPPORTED_MEDIA"),
            "got: {error}"
        );
    }

    /// With document routing in `shadow` the edge answers 200 for a PDF with
    /// no text and says what it saw. That is not a client-rendered page.
    #[tokio::test]
    async fn a_non_html_body_with_no_text_does_not_escalate() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "status": 200,
                    "markdown": "",
                    "metadata": {"content_kind": "pdf", "content_kind_source": "magic"},
                }
            })))
            .expect(1)
            .mount(&quarry)
            .await;

        let result = test_client(quarry.uri())
            .scrape_readable("https://example.com/a.pdf", "org-a", false)
            .await
            .expect("scrape succeeds");
        assert!(result.text.is_empty());
    }

    /// The counterpart: an HTML page whose static markup is empty because it
    /// renders on the client must still get its one browser retry, with or
    /// without ZDR, even though the response now says `content_kind: html`.
    #[tokio::test]
    async fn an_empty_html_page_still_escalates_once() {
        for zdr in [false, true] {
            let quarry = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/scrape"))
                .and(body_json(serde_json::json!({
                    "url": "https://spa.example",
                    "zdr": zdr,
                    "formats": ["markdown"],
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {
                        "status": 200,
                        "markdown": "",
                        "metadata": {"content_kind": "html"},
                    }
                })))
                .expect(1)
                .mount(&quarry)
                .await;
            Mock::given(method("POST"))
                .and(path("/v1/scrape"))
                .and(body_json(serde_json::json!({
                    "url": "https://spa.example",
                    "render": {"waitForSelector": "body", "waitForTimeoutMs": 2_500},
                    "signals": {
                        "actions": [],
                        "screenshot": false,
                        "pdf": false,
                        "prior_block_signals": 0,
                        "profile_required": true,
                        "url_type": "Default",
                    },
                    "zdr": zdr,
                    "formats": ["markdown"],
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {
                        "status": 200,
                        "markdown": "# Rendered",
                        "metadata": {"content_kind": "html"},
                    }
                })))
                .expect(1)
                .mount(&quarry)
                .await;

            let result = test_client(quarry.uri())
                .scrape_readable("https://spa.example", "org-a", zdr)
                .await
                .expect("scrape succeeds");
            assert_eq!(result.text, "# Rendered", "zdr = {zdr}");
        }
    }

    /// An empty `formats` list is left off the wire rather than sent as `[]`,
    /// which the edge would read as "no formats".
    #[tokio::test]
    async fn empty_formats_are_omitted_from_the_request() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/scrape"))
            .and(body_json(
                serde_json::json!({"url": "https://example.com", "zdr": false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(None)))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .scrape_with_options(
                "https://example.com",
                "org-a",
                &ScrapeOptions {
                    formats: Vec::new(),
                    ..ScrapeOptions::default()
                },
            )
            .await
            .expect("scrape succeeds");
    }

    #[test]
    fn project_prefers_top_level_markdown_over_the_legacy_formats_string() {
        let data = serde_json::json!({
            "status": 200,
            "markdown": "# new",
            "metadata": {"content_kind": "html"},
            "formats": {"markdown": "# old"},
        });
        let r = project("https://example.com", &data);
        assert_eq!(r.markdown, "# new");
        assert_eq!(r.text, "# new");
        assert_eq!(content_kind(&r.raw), Some("html"));
    }

    #[test]
    fn project_leaves_a_format_ref_for_the_artifact_path() {
        let data = serde_json::json!({
            "status": 200,
            "omitted": ["markdown"],
            "formats": {"markdown": {"artifact_id": "art_01X", "bytes": 9}},
        });
        let r = project("https://example.com", &data);
        assert!(r.text.is_empty());
        assert!(artifact_ref(&data, "markdown").is_some());
        assert_eq!(content_kind(&r.raw), None);
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

    // --- the score kind and the rerank report (QB-T12) ---------------------

    fn calibration() -> Calibration {
        Calibration::from_knots(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]).expect("a valid table")
    }

    fn bare_result(url: &str) -> SearchResult {
        project_search_result(&serde_json::json!({"url": url, "title": "T", "rank": 1}))
    }

    /// The response Quarry sends when a caller asked for `cross_encoder.v1` and the rerank applied:
    /// the head carries `score`, the tail does not, and the response names the kind and the report.
    /// Built from `quarry-edge`'s `SearchResponse` and `serp::SearchResult` serialisation. It is not a
    /// captured live response; the live proof captures one.
    fn applied_payload() -> Value {
        serde_json::json!({
            "query": "havvind utbygging",
            "provider": "smart_router",
            "results": [
                {
                    "url": "https://www.nve.no/nytt/rapport",
                    "title": "Rapport 2026",
                    "snippet": "Utbygging til havs.",
                    "rank": 1,
                    "provider": "searxng",
                    "score": 0.91,
                    "highlights": ["Utbygging til havs"],
                    "engines": ["brave", "google"]
                },
                {
                    "url": "https://www.nrk.no/nyhet",
                    "title": "Nyhet",
                    "snippet": "Om havvind.",
                    "rank": 2,
                    "provider": "searxng"
                }
            ],
            "count": 2,
            "score_kind": "cross_encoder.v1",
            "rerank": {"status": "applied", "profile": "web-v1"}
        })
    }

    /// A gateway that holds a calibration asks Quarry for the cross-encoder kind, and the body is the
    /// legacy body plus that one key.
    #[tokio::test]
    async fn a_gateway_that_can_calibrate_asks_for_the_cross_encoder_kind() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "kpi norge",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
                "score_kinds": ["cross_encoder.v1"],
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        test_client(quarry.uri())
            .with_cross_encoder_scores(calibration())
            .search("kpi norge", 5, "research", "org-a", false)
            .await
            .expect("search succeeds");
    }

    #[test]
    fn an_applied_reply_is_read_through_the_calibration() {
        let table = calibration();
        let reply = reply_from_payload(&applied_payload(), Some(&table));
        let head = &reply.results[0];
        assert_eq!(
            head.relevance,
            table.apply(0.91),
            "the calibrated value, not the raw 0.91"
        );
        assert_eq!(head.score, table.apply(0.91).unwrap_or(0.0));
        assert_eq!(head.score_kind, Some(ScoreKind::CrossEncoderV1));
        let tail = &reply.results[1];
        assert_eq!(tail.relevance, None, "the unreranked tail is not judged");
        assert_eq!(tail.score_kind, None);
        assert_eq!(
            reply.rerank,
            Some(RerankOutcome {
                status: RerankStatus::Applied,
                reason: None
            })
        );
    }

    /// A ZDR skip carries no score on any hit, and the reason is what the coverage line will name.
    #[test]
    fn a_skipped_zdr_reply_carries_no_score_and_says_why() {
        let payload = serde_json::json!({
            "query": "kpi",
            "provider": "smart_router",
            "results": [{"url": "https://www.ssb.no/kpi", "title": "KPI", "snippet": "x", "rank": 1, "provider": "searxng", "score": 0.9}],
            "count": 1,
            "rerank": {"status": "skipped", "reason": "zdr", "profile": "web-v1"}
        });
        let reply = reply_from_payload(&payload, Some(&calibration()));
        assert_eq!(reply.results[0].relevance, None);
        assert_eq!(reply.results[0].score_kind, None);
        assert_eq!(
            reply.rerank,
            Some(RerankOutcome {
                status: RerankStatus::Skipped,
                reason: Some("zdr".to_owned())
            })
        );
    }

    #[test]
    fn a_degraded_reply_keeps_the_order_and_says_why() {
        let mut payload = applied_payload();
        payload["rerank"] =
            serde_json::json!({"status": "degraded", "reason": "timeout", "profile": "web-v1"});
        payload
            .as_object_mut()
            .expect("object")
            .remove("score_kind");
        let reply = reply_from_payload(&payload, Some(&calibration()));
        assert_eq!(
            reply.results[0].relevance, None,
            "a degraded rerank scores nothing"
        );
        assert_eq!(
            reply.rerank,
            Some(RerankOutcome {
                status: RerankStatus::Degraded,
                reason: Some("timeout".to_owned())
            })
        );
    }

    #[test]
    fn a_shadow_reply_is_named_and_carries_no_score() {
        let mut payload = applied_payload();
        payload["rerank"] =
            serde_json::json!({"status": "shadow", "reason": "would_apply", "profile": "web-v1"});
        payload
            .as_object_mut()
            .expect("object")
            .remove("score_kind");
        let reply = reply_from_payload(&payload, Some(&calibration()));
        assert_eq!(reply.results[0].relevance, None);
        assert_eq!(
            reply.rerank,
            Some(RerankOutcome {
                status: RerankStatus::Shadow,
                reason: Some("would_apply".to_owned())
            })
        );
    }

    /// The legacy path: a response that names no score kind and no rerank report reads exactly as it
    /// did before the score kind existed. The raw score is kept even when a table is present, because
    /// the table applies only to the kind it was fitted for.
    #[test]
    fn a_legacy_reply_is_read_exactly_as_it_always_was() {
        let payload = serde_json::json!({
            "results": [
                {"url": "https://a.no/", "title": "A", "rank": 1, "score": 0.7, "highlights": ["h"], "engines": ["brave", "google"]},
                {"url": "https://b.no/", "title": "B", "rank": 2}
            ],
            "count": 2
        });
        let reply = reply_from_payload(&payload, Some(&calibration()));
        assert_eq!(reply.results[0].relevance, Some(0.7));
        assert!((reply.results[0].score - 0.7).abs() < f32::EPSILON);
        assert_eq!(reply.results[0].score_kind, None);
        assert_eq!(reply.results[1].relevance, None);
        assert_eq!(reply.rerank, None);
    }

    /// A cross-encoder answer read with no table is absent. The raw score must never reach the gate;
    /// the report, which is still true, is kept.
    #[test]
    fn a_cross_encoder_reply_without_a_table_is_absent() {
        let reply = reply_from_payload(&applied_payload(), None);
        assert_eq!(reply.results[0].relevance, None);
        assert_eq!(reply.results[0].score_kind, None);
        assert!(reply.results[0].score.abs() < f32::EPSILON);
        assert_eq!(
            reply.rerank.map(|outcome| outcome.status),
            Some(RerankStatus::Applied)
        );
    }

    #[test]
    fn an_unknown_score_kind_is_never_consumed() {
        let mut payload = applied_payload();
        payload["score_kind"] = serde_json::json!("cross_encoder.v2");
        let reply = reply_from_payload(&payload, Some(&calibration()));
        assert_eq!(reply.results[0].relevance, None);
        assert_eq!(reply.results[0].score_kind, None);
    }

    #[test]
    fn a_rerank_status_this_gateway_does_not_name_is_no_signal() {
        for status in ["rerank-not-configured", "partial"] {
            let mut payload = applied_payload();
            payload["rerank"] = serde_json::json!({"status": status});
            assert_eq!(reply_from_payload(&payload, None).rerank, None, "{status}");
        }
        let mut payload = applied_payload();
        payload.as_object_mut().expect("object").remove("rerank");
        assert_eq!(
            reply_from_payload(&payload, None).rerank,
            None,
            "an absent report"
        );
    }

    /// A report this gateway does not name is not a legacy answer, so a score that came with it is not
    /// read: the gate cannot tell whether the rerank behind the score stood.
    #[test]
    fn a_score_that_came_with_an_unnamed_rerank_report_is_not_read() {
        for status in ["rerank-not-configured", "partial"] {
            let mut payload = applied_payload();
            payload
                .as_object_mut()
                .expect("object")
                .remove("score_kind");
            payload["rerank"] = serde_json::json!({"status": status});
            let reply = reply_from_payload(&payload, Some(&calibration()));
            assert_eq!(reply.results[0].relevance, None, "{status}");
            assert_eq!(reply.results[0].score, 0.0, "{status}");
        }
    }

    /// A calibrated zero is a judgement and reaches the gate as a score. A legacy zero is "not judged",
    /// the rule the gate has always had.
    #[test]
    fn a_calibrated_zero_is_a_judgement_and_a_legacy_zero_is_not() {
        let mut calibrated = bare_result("https://a.no/");
        calibrated.relevance = Some(0.0);
        calibrated.score_kind = Some(ScoreKind::CrossEncoderV1);
        assert_eq!(calibrated.wire_score(), Some(0.0));

        let mut legacy = bare_result("https://b.no/");
        legacy.relevance = Some(0.0);
        assert_eq!(legacy.wire_score(), None);

        let mut legacy_judged = bare_result("https://c.no/");
        legacy_judged.relevance = Some(0.3);
        legacy_judged.score = 0.3;
        assert_eq!(legacy_judged.wire_score(), Some(0.3));

        assert_eq!(bare_result("https://d.no/").wire_score(), None);
    }

    /// The envelope's provider rides on the reply, so the disclosure line can name a search that found nothing
    /// (EP-9 W1). An edge that names none leaves it empty.
    #[test]
    fn the_reply_carries_the_envelope_provider() {
        let answered = reply_from_payload(
            &serde_json::json!({"provider": "smart_router", "results": []}),
            None,
        );
        assert_eq!(answered.provider, "smart_router");
        assert!(answered.results.is_empty());
        assert_eq!(
            reply_from_payload(&serde_json::json!({"results": []}), None).provider,
            ""
        );
    }

    /// The legacy hit keeps the keys it always had with the same values, and what carries nothing is
    /// omitted, so the gate can tell "the provider said nothing" from "the provider said zero".
    #[test]
    fn a_legacy_web_search_item_keeps_its_keys_and_omits_what_carries_nothing() {
        let payload = serde_json::json!({"results": [
            {"url": "https://a.no/", "title": "A", "snippet": "S", "rank": 1, "score": 0.7, "highlights": ["h"], "engines": ["brave", "google"]},
            {"url": "https://b.no/", "title": "B", "snippet": "", "score": 0.0}
        ]});
        let reply = reply_from_payload(&payload, None);
        assert_eq!(
            web_search_item(&reply.results[0], None),
            serde_json::json!({
                "url": "https://a.no/",
                "title": "A",
                "snippet": "S",
                "rank": 1,
                "score": 0.7_f32,
                "highlights": ["h"],
                "engines": ["brave", "google"]
            })
        );
        assert_eq!(
            web_search_item(&reply.results[1], None),
            serde_json::json!({"url": "https://b.no/", "title": "B", "snippet": ""}),
            "a legacy zero and an empty list are omitted"
        );
    }

    #[test]
    fn a_web_search_item_names_the_rerank_report_it_came_with() {
        let result = bare_result("https://a.no/");
        let skipped = RerankOutcome {
            status: RerankStatus::Skipped,
            reason: Some("zdr".to_owned()),
        };
        let item = web_search_item(&result, Some(&skipped));
        assert_eq!(item["rerank_status"], "skipped");
        assert_eq!(item["rerank_reason"], "zdr");

        let shadow = RerankOutcome {
            status: RerankStatus::Shadow,
            reason: None,
        };
        let item = web_search_item(&result, Some(&shadow));
        assert_eq!(item["rerank_status"], "shadow");
        assert!(item.get("rerank_reason").is_none());
        assert!(web_search_item(&result, None)
            .get("rerank_status")
            .is_none());
    }

    #[test]
    fn a_calibrated_zero_reaches_the_gate_as_a_score() {
        let mut result = bare_result("https://a.no/");
        result.relevance = Some(0.0);
        result.score_kind = Some(ScoreKind::CrossEncoderV1);
        assert_eq!(web_search_item(&result, None)["score"], 0.0);
    }

    #[test]
    fn the_rerank_words_are_the_words_quarry_sends() {
        assert_eq!(RerankStatus::Applied.label(), "applied");
        assert_eq!(RerankStatus::Skipped.label(), "skipped");
        assert_eq!(RerankStatus::Degraded.label(), "degraded");
        assert_eq!(RerankStatus::Shadow.label(), "shadow");
    }

    #[test]
    fn attribution_switch_is_on_only_for_the_literal_true() {
        assert!(attribution::parse_enabled(Some("true")));
        let not_on = [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some("on"),
            Some(" true"),
        ];
        for raw in not_on {
            assert!(!attribution::parse_enabled(raw), "{raw:?}");
        }
    }

    /// The request a Quarry read builds inside an attribution scope, with the switch given.
    async fn attributed_request(on: bool) -> reqwest::Request {
        let scoped = attribution::Attribution {
            request_id: Some("req_1".to_owned()),
            parent_run: Some("run_1".to_owned()),
        };
        attribution::scope_when(on, scoped, async {
            attribution::apply(reqwest::Client::new().get("http://127.0.0.1/"))
                .build()
                .expect("the request builds")
        })
        .await
    }

    #[tokio::test]
    async fn attribution_headers_go_out_only_when_the_switch_is_on() {
        let off = attributed_request(false).await;
        assert!(off.headers().get("x-request-id").is_none());
        assert!(off.headers().get("x-verevon-parent-run").is_none());

        let on = attributed_request(true).await;
        assert_eq!(on.headers()["x-request-id"].to_str().unwrap(), "req_1");
        assert_eq!(
            on.headers()["x-verevon-parent-run"].to_str().unwrap(),
            "run_1"
        );
    }
}
