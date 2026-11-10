//! /v1/search — SERP-backed discovery (QRY-12) + Tavily-parity params (P0 1A).
//!
//! Wraps the runtime's `SearchProvider` trait. The edge route normalizes
//! request shape, enforces ZDR (search queries are control-plane signals
//! and never persisted, so ZDR=on is fine), and returns ranked URLs ready
//! for crawl seeding via `/v1/crawl` or one-shot scrape.
//!
//! Tavily-parity options (for Verevon's search/answer UX):
//!   * `topic` (general|news|finance), `time_range`/`days` recency window,
//!   * `exact_match` (quoted-phrase), `chunks_per_source`,
//!   * `include_answer` (fuse `/v1/answer`), `format=context` (token-bounded
//!     RAG context string).
//!
//! When no provider is configured, returns 501 Unsupported with a hint.
//!
//! Structured facts: with `include_answer`, each citation may carry the
//! machine-readable harvest from its page (`Citation::structured`) — the
//! channel that recovers figures readability strips with the `script` tags
//! they live in. It is page content, so it travels exactly as the synthesized
//! answer does: through the org-keyed cache below, which a ZDR request
//! bypasses on both read and write, and no further.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

use quarry_runtime::answer::{AnswerRequest, Citation, MarkdownFetcher, SimpleHttpMarkdownFetcher};
use quarry_runtime::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use quarry_runtime::rerank::{RerankEpoch, RerankReport, RerankStatus};
use quarry_runtime::serp::{
    ImageResult, SearXNGImages, SearXNGVideos, SearchOptions, SearchResult, VideoResult,
};
use quarry_runtime::smart_router::{classify_intent, QueryIntent};
use quarry_runtime::vector_index::{VectorHit, VectorOutcome, VectorQuery};

use quarry_core::zdr::ZdrMode;
use quarry_runtime::answer::FetchFailure;
use quarry_transform::structured::StructuredData;
use quarry_runtime::job_pricing::{
    estimate_job_price, BulkJobKind, JobPrice, JobPriceInput, JobRates,
};

use crate::answer_status::{self, AnswerReason, AnswerStatus};
use crate::state::AppState;

/// Char budget for `format=context` (~2k tokens at ~4 chars/token).
const MAX_CONTEXT_CHARS: usize = 8_000;
/// Cap on sources scraped when `include_answer` is set.
const ANSWER_TOP_K: usize = 5;
/// Max chars of a fetched page used as the find-similar query seed. The whole
/// page would bloat the embedding call; the lead is the strongest signal.
const SIMILAR_SEED_MAX_CHARS: usize = 2_000;
/// The score kind the Data Plane's cross-encoder produces (`rerank-v1`). A caller opts in to it.
const CROSS_ENCODER_V1: &str = "cross_encoder.v1";
/// Every score kind the edge can emit. Any other name in `score_kinds` is refused.
const SCORE_KINDS: &[&str] = &[CROSS_ENCODER_V1];

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Zero Data Retention: bypass caches and durable content-bearing events.
    /// The request can only raise the posture: a user of a ZDR organisation
    /// runs as ZDR whatever this says (see `Claims::zdr_floor`). Absent means
    /// `false` for a user. A service caller must send it (`ZDR_REQUIRED`): its
    /// own token's `zdr` claim is never read, so its body is its posture.
    #[serde(default)]
    pub zdr: Option<bool>,
    /// Per-request permission to dispatch the paid external SERP providers
    /// (Brave, Serper) — money and egress, so it is a permission the caller
    /// has to assert, never an inference from anything else on the request.
    ///
    /// A plain `bool` with `#[serde(default)]` rather than `Option<bool>`:
    /// absent means `false` means no paid fan-out, and a non-boolean value
    /// fails deserialization so axum answers 422 instead of coercing it.
    /// Missing input and malformed input therefore both land closed, which is
    /// the only safe posture for a spend permission.
    ///
    /// Deliberately NOT given the warn-and-drop leniency `intent` gets below:
    /// silently downgrading an unreadable permission to "granted" is a leak and
    /// to "denied" is a silent behaviour change the caller never sees — a 422
    /// says which of the two happened.
    #[serde(default)]
    pub allow_paid_providers: bool,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default = "default_safe_search")]
    pub safe_search: bool,
    /// Topic vertical: "general" | "news" | "finance".
    #[serde(default)]
    pub topic: Option<String>,
    /// Recency window: "day" | "week" | "month" | "year".
    #[serde(default)]
    pub time_range: Option<String>,
    /// Recency in days — convenience alias; converted to a `time_range` bucket.
    #[serde(default)]
    pub days: Option<u32>,
    /// Treat the query as an exact phrase (quoted).
    #[serde(default)]
    pub exact_match: bool,
    /// Max snippets retained per source host when building context.
    #[serde(default)]
    pub chunks_per_source: Option<u32>,
    /// Also synthesize an answer (fuses `/v1/answer`); requires answer pipeline.
    #[serde(default)]
    pub include_answer: bool,
    /// Output format: "results" (default) or "context" (RAG context string).
    #[serde(default)]
    pub format: Option<String>,
    /// Wrap query terms in `<mark>…</mark>` within result titles/snippets.
    #[serde(default)]
    pub highlight: bool,
    /// Include result aggregations (facet counts by host).
    #[serde(default)]
    pub facets: bool,
    /// Restrict results to these domains (Exa-style `includeDomains`). Applied
    /// as `site:` operators across the SERP providers.
    #[serde(default)]
    pub include_domains: Vec<String>,
    /// Exclude these domains (Exa-style `excludeDomains`). Applied as `-site:`
    /// operators across the SERP providers.
    #[serde(default)]
    pub exclude_domains: Vec<String>,
    /// Caller-supplied query-intent hint. Verevon's model-gateway stamps this
    /// on every `/v1/search` call; until this field existed serde discarded it
    /// with no error on either side, so the hint looked wired end-to-end while
    /// changing nothing about routing.
    ///
    /// It is now forwarded: parsed against the router's own `QueryIntent`
    /// vocabulary and carried to the runtime in `SearchOptions::intent`, where
    /// `SmartSearchRouter::resolve_intent` adopts it as an *override* of its
    /// own classification and skips the classifier call entirely. The rules
    /// are therefore never consulted on a hinted request, so the edge runs
    /// them itself and emits both verdicts: that comparison is the
    /// caller-versus-rules disagreement rate, and once the hint wins it exists
    /// nowhere else.
    ///
    /// The hint also joins the edge's cache signature. It has to: it changes
    /// which engines run, so two requests differing only by intent produce
    /// different result sets and would otherwise be served each other's.
    ///
    /// Unrecognized values are logged and dropped rather than rejected:
    /// promoting a field that every deployed model-gateway build already sends
    /// (and that was silently ignored until now) into a 400 would turn a no-op
    /// into an outage. That leniency belongs to an advisory hint only — see
    /// `allow_paid_providers` above, where the same treatment would be wrong.
    #[serde(default)]
    pub intent: Option<String>,
    /// The score kinds the caller accepts on the results. Empty (the default) means
    /// the order only: no score reaches the wire. `cross_encoder.v1` asks for the Data
    /// Plane's cross-encoder scores on a reranked result set, and the response then
    /// names the kind in `score_kind`. An unknown kind is refused, not ignored.
    #[serde(default)]
    pub score_kinds: Vec<String>,
    /// E3.7.7a: hydrate the top results through the scrape path (see [`SearchScrapeBlock`]).
    /// Read only when `QUARRY_EDGE__SEARCH_SCRAPE` is on; off, the block is ignored and the
    /// response carries `scrape.status = "disabled"`, so a dark edge never changes a search.
    #[serde(default)]
    pub scrape: Option<SearchScrapeBlock>,
    /// Optional client-supplied request ID for tracing.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub request_id: Option<String>,
}

fn default_safe_search() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub provider: String,
    pub results: Vec<SearchResult>,
    pub count: usize,
    /// Present when `include_answer=true` and synthesis succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// INNH-8a: present only when `include_answer=true`. Says whether `answer` is a real answer
    /// (`ok`), empty because no source had text (`no_sources`), withheld under zero data
    /// retention (`skipped_zdr`) or absent (`unavailable`, with `answer_reason`). A request that
    /// did not ask for an answer carries neither field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_status: Option<AnswerStatus>,
    /// INNH-8a: the closed reason of an `unavailable` status (`synthesis_disabled`,
    /// `no_search_provider`, `no_model_plane_url`, `model_client_init_failed`,
    /// `model_plane_auth_refused`, `model_plane_rate_limited`, `model_plane_error`,
    /// `answer_search_failed`). Never provider text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_reason: Option<AnswerReason>,
    /// Sources behind `answer`. A citation additionally carries that page's
    /// structured harvest when it had one — see the module header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<Citation>>,
    /// Present when `format=context` — token-bounded RAG context string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Present when `facets=true` — result aggregations by host (desc count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<Vec<FacetCount>>,
    /// WS7.E: the permissions the web policy took away from this request, by capability
    /// name. Present only when the request asked for one the policy refused; the search
    /// ran without it. A downgrade is never a 403.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub policy_downgrades: Vec<String>,
    /// The kind of the scores on `results`. Present only when the caller asked for
    /// that kind in `score_kinds` and the rerank was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_kind: Option<&'static str>,
    /// What the rerank step did for these results. Absent when no rerank step is
    /// wired in, so a deployment without reranking keeps the response it had.
    #[serde(skip_serializing_if = "RerankReport::is_not_configured")]
    pub rerank: RerankReport,
    /// E3.7.7a: the hydration of the top results. Present only when the request sent a
    /// `scrape` block. Page text travels inline in this response only: it is never cached,
    /// never written to an event and never logged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrape: Option<SearchScrapeReport>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FacetCount {
    pub value: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Default TTL (seconds) for the general search/answer cache bucket. News /
/// recency-scoped queries override this downward — see `cache::search_ttl_secs`.
const SEARCH_CACHE_DEFAULT_TTL_SECS: u64 = 900;

/// Cached upstream of a search: provider results + any synthesized answer.
/// Post-processing (facets/highlight/context) is re-applied per request, and
/// billing/events still fire on cache hits — only the expensive provider + LLM
/// calls are skipped. Keyed org-scoped so one tenant never reads another's.
#[derive(Serialize, Deserialize)]
struct CachedSearch {
    provider: String,
    results: Vec<SearchResult>,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    citations: Option<Vec<Citation>>,
    /// The rerank outcome the entry was written with, so a hit reports the same
    /// status as the miss that filled it. Entries from before the field existed
    /// read back as `rerank-not-configured`.
    #[serde(default)]
    rerank: RerankReport,
}

// ── intent hint + source-quality observability (unit-tested) ────────────────
//
// Everything below emits through `tracing`, not the `metrics` facade. That is
// deliberate: a recorder is only installed when `QUARRY_EDGE__METRICS_ENABLED=1`
// (see `crate::metrics`), so a `metrics::counter!` here would be a silent no-op
// in every deployment that leaves it off, and would need its own closed label
// set when on. `tracing` events reach the JSON fmt layer and, when
// `OTEL_EXPORTER_OTLP_ENDPOINT` is set, the OTLP exporter — see
// `telemetry::init_telemetry`.
//
// Cardinality discipline for every field emitted from the search path: no
// query text, no URLs, no org/user ids. Engine names are clamped to
// `KNOWN_SEARCH_ENGINES` and intents to `intent_label`, both closed sets.

/// Map a caller's `intent` string onto the router's own [`QueryIntent`]
/// vocabulary. Built on the runtime enum rather than a local string list so
/// the accepted vocabulary cannot drift from what the router actually routes
/// on. Aliases cover the spellings model-gateway is known to send.
pub(crate) fn parse_intent_hint(raw: &str) -> Option<QueryIntent> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "navigational" | "nav" => Some(QueryIntent::Navigational),
        "fresh" | "news" | "recent" => Some(QueryIntent::Fresh),
        "phrase" | "exact" => Some(QueryIntent::Phrase),
        "research" | "deep_research" => Some(QueryIntent::Research),
        "comparative" | "compare" => Some(QueryIntent::Comparative),
        "local" => Some(QueryIntent::Local),
        "code" => Some(QueryIntent::Code),
        "default" | "general" => Some(QueryIntent::Default),
        _ => None,
    }
}

/// Stable telemetry label for a [`QueryIntent`]. An explicit match rather than
/// `{:?}` so a rename or a new variant in the runtime enum breaks the build
/// here instead of quietly renaming a dimension that dashboards group by.
pub(crate) fn intent_label(intent: QueryIntent) -> &'static str {
    match intent {
        QueryIntent::Navigational => "navigational",
        QueryIntent::Fresh => "fresh",
        QueryIntent::Phrase => "phrase",
        QueryIntent::Research => "research",
        QueryIntent::Comparative => "comparative",
        QueryIntent::Local => "local",
        QueryIntent::Code => "code",
        QueryIntent::Default => "default",
    }
}

/// What the caller's `intent` hint actually did to this request.
///
/// Deliberately not a boolean. `SmartSearchRouter::resolve_intent` treats a
/// supplied hint as an override of its own classification — it does not seed a
/// classifier that may then disagree, it skips the classifier outright — so
/// "was the hint honored" is yes by construction for every hint that parses,
/// and a bool would do nothing but restate `intent_hint`. The question that
/// still has an answer, and the one this field was added for, is whether the
/// caller agreed with the rules or displaced them: a climbing `used_overrode`
/// rate is the caller and the rule classifier drifting apart.
///
/// `supplied` is whether the request carried a non-blank `intent` at all and
/// `hint` is the parse of it, kept as separate arguments so a hint that
/// arrived but could not be read stays distinguishable from no hint — both
/// leave the router classifying for itself, but only one is a client bug.
///
/// Four values, fixed here, so the dimension stays groupable.
pub(crate) fn intent_hint_effect(
    supplied: bool,
    hint: Option<QueryIntent>,
    rule: QueryIntent,
) -> &'static str {
    match hint {
        // Dropped at the edge before `SearchOptions` was built, so the router
        // classified this request itself. Same routing as an absent hint, but
        // a contract breach rather than a choice — counting them together
        // would hide a mis-wired client inside the no-hint baseline.
        None if supplied => "invalid",
        None => "none",
        Some(h) if h == rule => "used_agreed",
        Some(_) => "used_overrode",
    }
}

/// Engines a result can be attributed to. `SearchResult.provider` is filled in
/// by the provider adapters, so clamping to this closed list is what keeps the
/// `engine` dimension bounded if an adapter ever starts writing a per-request
/// string there. A genuinely new engine reads as `other` until it is added
/// here — bounded and vague beats unbounded and precise for a label.
const KNOWN_SEARCH_ENGINES: [&str; 7] = [
    "brave",
    "hybrid",
    "lex",
    "searxng",
    "serper",
    "stract",
    "tantivy_local",
];

pub(crate) fn search_engine_label(raw: &str) -> &'static str {
    KNOWN_SEARCH_ENGINES
        .into_iter()
        .find(|known| *known == raw)
        .unwrap_or("other")
}

/// Snippet-length distribution across one result set, in chars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SnippetStats {
    pub min: usize,
    pub median: usize,
    pub max: usize,
    /// Results that carried no snippet (or a blank one). A CAPTCHA'd or
    /// rate-limited engine usually still returns links, so this is the field
    /// that separates "upstream answered" from "upstream answered usefully".
    pub missing: usize,
}

/// Summarise snippet lengths. Blank snippets count as missing rather than as
/// length 0, otherwise a shut-out engine would drag `min` to 0 and look like a
/// terse-but-working one.
pub(crate) fn snippet_stats<'a, I>(snippets: I) -> SnippetStats
where
    I: IntoIterator<Item = Option<&'a str>>,
{
    let mut lens: Vec<usize> = Vec::new();
    let mut missing = 0usize;
    for snippet in snippets {
        match snippet.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => lens.push(s.chars().count()),
            None => missing += 1,
        }
    }
    if lens.is_empty() {
        return SnippetStats {
            missing,
            ..Default::default()
        };
    }
    lens.sort_unstable();
    SnippetStats {
        min: lens[0],
        // Lower median on even counts — this is a health signal, not a
        // statistic anyone does arithmetic on downstream.
        median: lens[lens.len() / 2],
        max: lens[lens.len() - 1],
        missing,
    }
}

/// Render the per-engine contribution as one `engine=count` field, alongside
/// (not instead of) the per-engine events. An engine that returns nothing
/// emits no contribution event at all, so per-engine events alone cannot
/// distinguish "three upstreams are quiet today" from "three upstreams have
/// been CAPTCHA'd for a month" — which is exactly how this went unnoticed.
/// Carrying the whole distribution on the shape event makes the width of the
/// chain visible on every single request. `BTreeMap` ordering keeps the
/// string stable across requests so it can be grouped on.
pub(crate) fn render_engine_mix(mix: &std::collections::BTreeMap<&'static str, usize>) -> String {
    mix.iter()
        .map(|(engine, hits)| format!("{engine}={hits}"))
        .collect::<Vec<_>>()
        .join(",")
}

// ── pure helpers (unit-tested) ──────────────────────────────────────────────

/// Wrap the query in double-quotes for exact-phrase matching, unless it is
/// already a single quoted phrase. Lexical providers do phrase matching; SERP
/// providers (Brave/Serper/SearXNG) honor quotes natively.
pub(crate) fn apply_exact_match(query: &str, exact: bool) -> String {
    let q = query.trim();
    if !exact || (q.starts_with('"') && q.ends_with('"') && q.len() >= 2) {
        return q.to_string();
    }
    format!("\"{}\"", q.replace('"', ""))
}

/// Resolve the effective recency window from an explicit `time_range` or a
/// `days` count. Explicit `time_range` wins; otherwise `days` is bucketed.
pub(crate) fn derive_time_range(time_range: Option<&str>, days: Option<u32>) -> Option<String> {
    if let Some(tr) = time_range {
        let tr = tr.trim().to_lowercase();
        if matches!(tr.as_str(), "day" | "week" | "month" | "year") {
            return Some(tr);
        }
    }
    match days {
        Some(d) if d <= 1 => Some("day".into()),
        Some(d) if d <= 7 => Some("week".into()),
        Some(d) if d <= 31 => Some("month".into()),
        Some(_) => Some("year".into()),
        None => None,
    }
}

/// Build a token-bounded RAG context string from ranked results. Caps snippets
/// per source host at `chunks_per_source` (when set) and stops at `max_chars`.
pub(crate) fn build_context(
    results: &[SearchResult],
    chunks_per_source: Option<u32>,
    max_chars: usize,
) -> String {
    use std::collections::HashMap;
    let per_host_cap = chunks_per_source.map(|c| c as usize);
    let mut per_host: HashMap<String, usize> = HashMap::new();
    let mut out = String::new();
    let mut n = 0usize;
    for r in results {
        let host = url::Url::parse(&r.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if let Some(cap) = per_host_cap {
            let c = per_host.entry(host.clone()).or_insert(0);
            if *c >= cap {
                continue;
            }
            *c += 1;
        }
        n += 1;
        let title = r.title.as_deref().unwrap_or("");
        let snippet = r.snippet.as_deref().unwrap_or("");
        let block = format!("[{n}] {title}\n{snippet}\n({})\n\n", r.url);
        if out.len() + block.len() > max_chars {
            break;
        }
        out.push_str(&block);
    }
    out.trim_end().to_string()
}

/// Reduce host crowding ("result diversity"): keep at most `MAX_PER_HOST`
/// results from any single host in the primary block; results beyond that cap
/// from a dominant host are demoted to the tail (still returned, never dropped).
/// Stable — intra-host order is preserved, and results with no parseable host
/// are never capped. Applied outermost (after merge/rerank) so one domain can't
/// monopolise the visible results. Pure → unit-tested.
///
/// Returns the reordered set plus how many results the cap demoted. The count
/// is source-quality telemetry, not a return value the response shape uses: a
/// query whose whole first page comes from one host means the rest of the
/// engine chain contributed nothing, which is invisible from the result count
/// alone (the demoted results are still in there, just at the tail).
pub(crate) fn diversify_results(results: Vec<SearchResult>) -> (Vec<SearchResult>, usize) {
    use std::collections::HashMap;
    const MAX_PER_HOST: usize = 3;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut kept: Vec<SearchResult> = Vec::with_capacity(results.len());
    let mut deferred: Vec<SearchResult> = Vec::new();
    for result in results {
        let host = url::Url::parse(&result.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned));
        match host {
            Some(h) => {
                let count = counts.entry(h).or_insert(0);
                if *count < MAX_PER_HOST {
                    *count += 1;
                    kept.push(result);
                } else {
                    deferred.push(result);
                }
            }
            None => kept.push(result),
        }
    }
    let demoted = deferred.len();
    kept.extend(deferred);
    (kept, demoted)
}

/// Assemble the runtime [`SearchOptions`] for one `/v1/search` request.
///
/// Extracted from the handler so the two fail-closed decisions it makes — the
/// paid-provider permission ANDed with `!zdr`, and the org scope taken from the
/// verified JWT rather than from the body — are reachable from a unit test
/// instead of only from a live route.
pub(crate) fn build_search_options(
    req: &SearchRequest,
    org_id: &str,
    zdr: bool,
    intent_hint: Option<QueryIntent>,
) -> SearchOptions {
    SearchOptions {
        limit: req.limit.unwrap_or(10).min(50),
        country: req.country.clone(),
        language: req.language.clone(),
        safe_search: req.safe_search,
        topic: req.topic.clone(),
        time_range: derive_time_range(req.time_range.as_deref(), req.days),
        exact_match: req.exact_match,
        // Tenant isolation: thread the verified JWT org_id so private-corpus
        // providers (TantivyLocalIndex) restrict to this org.
        org_id: Some(org_id.to_string()),
        include_domains: req.include_domains.clone(),
        exclude_domains: req.exclude_domains.clone(),
        // ZDR: SmartSearchRouter must not send this query to an external
        // paid SERP SaaS provider (Brave/Serper) when the caller flagged
        // the request zero-retention. See `SearchOptions::zdr`.
        zdr,
        // Granted only when the caller asked for it AND the request is not
        // zero-retention, written as one conjunction so the ZDR half cannot be
        // mistaken for something that happens elsewhere. The router applies its
        // own independent `!opts.zdr` test at every paid dispatch site; this is
        // the outer of two belts, never a substitute for it.
        allow_paid_providers: req.allow_paid_providers && !zdr,
        intent: intent_hint,
    }
}

/// Render the result-affecting parameters into the edge cache key's signature
/// segment. `highlight` / `facets` / `format` are deliberately absent — they
/// are re-applied per request on a hit, so those variants share one entry.
///
/// `intent` is present because it steers which engines the router dispatches:
/// omitting it would let a `research` request be served the cached result set
/// of a `navigational` one for the same query string.
///
/// `rerank` is the epoch the provider chain searches under. An entry written
/// under one rerank configuration must not be served under another.
///
/// The paid-provider permission enters through paid_allowed: paid providers change
/// which results come back, so a request that may not use them must not read an
/// entry that a request which may has written.
pub(crate) fn params_signature(
    opts: &SearchOptions,
    include_answer: bool,
    rerank: &RerankEpoch,
) -> String {
    format!(
        "l={}|c={:?}|lg={:?}|s={}|t={:?}|tr={:?}|x={}|a={}|inc={:?}|exc={:?}|i={}|pp={}|rr={}",
        opts.limit,
        opts.country,
        opts.language,
        opts.safe_search,
        opts.topic,
        opts.time_range,
        opts.exact_match,
        include_answer,
        opts.include_domains,
        opts.exclude_domains,
        opts.intent.map(intent_label).unwrap_or("none"),
        opts.paid_allowed(),
        rerank.signature(),
    )
}

/// Whether a search outcome may be written to the edge cache. Only outcomes that
/// are final for the epoch in the key are stored. A `degraded` or `skipped`
/// rerank is a transient state of the reranker, and caching it would pin
/// unreranked results for the whole TTL after the reranker recovered. A ZDR
/// request never writes.
pub(crate) fn cacheable_search(zdr: bool, rerank: &RerankReport) -> bool {
    !zdr && rerank.status.is_cacheable()
}

/// What the caller may see of the scores, and the kind the response names. A score reaches the
/// wire only for a caller that asked for its kind in `score_kinds`, and only on a reranked set
/// (`applied`). Every other caller gets the order alone, so the scores that a relevance gate
/// reads are never a surprise. Returns the kind to report, if any. Pure, so the policy is tested
/// apart from the route.
pub(crate) fn apply_score_policy(
    results: &mut [SearchResult],
    rerank: &RerankReport,
    score_kinds: &[String],
) -> Option<&'static str> {
    let asked = score_kinds.iter().any(|kind| kind == CROSS_ENCODER_V1);
    if asked && rerank.status == RerankStatus::Applied {
        return Some(CROSS_ENCODER_V1);
    }
    // Every other outcome carries the order alone: a caller that did not ask gets no score, and a
    // caller that asked gets none from a rerank that did not apply (shadow, skipped or degraded).
    for result in results.iter_mut() {
        result.score = None;
    }
    None
}

/// Aggregate results by host into descending-count facets (ties broken by host
/// name). Provider-agnostic — works on any `SearchResult` set.
pub(crate) fn compute_host_facets(results: &[SearchResult]) -> Vec<FacetCount> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in results {
        if let Some(h) = url::Url::parse(&r.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
        {
            *counts.entry(h).or_insert(0) += 1;
        }
    }
    let mut v: Vec<FacetCount> = counts
        .into_iter()
        .map(|(value, count)| FacetCount { value, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    v
}

/// Wrap each query term (≥2 chars) in `<mark>…</mark>` within `text`,
/// case-insensitively, longest-term-first to reduce nesting. ASCII fast-path
/// keeps byte indices aligned; non-ASCII falls back to a case-sensitive
/// replace to stay UTF-8-boundary-safe.
pub(crate) fn highlight_terms(text: &str, query: &str) -> String {
    let q = query.trim().trim_matches('"');
    let mut terms: Vec<&str> = q
        .split_whitespace()
        .filter(|t| t.chars().count() >= 2)
        .collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let mut out = text.to_string();
    for term in terms {
        out = wrap_ci(&out, term);
    }
    out
}

fn wrap_ci(text: &str, term: &str) -> String {
    if !text.is_ascii() || !term.is_ascii() {
        return text.replace(term, &format!("<mark>{term}</mark>"));
    }
    let lower = text.to_ascii_lowercase();
    let lt = term.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len() + 13);
    let (mut last, mut from) = (0usize, 0usize);
    while let Some(rel) = lower[from..].find(&lt) {
        let start = from + rel;
        let end = start + lt.len();
        result.push_str(&text[last..start]);
        result.push_str("<mark>");
        result.push_str(&text[start..end]);
        result.push_str("</mark>");
        last = end;
        from = end;
    }
    result.push_str(&text[last..]);
    result
}

// ── handler ─────────────────────────────────────────────────────────────────

/// `POST /v1/search`. WS4.J: wraps `search_run` so a ZDR run's metadata-only tombstone is
/// sent once, from the response status, whichever of the handler's returns produced it
/// (off unless the flag is on; fail open).
pub async fn search(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SearchRequest>,
) -> axum::response::Response {
    let pending = crate::zdr_tombstone::Pending::begin(
        state.tombstones.as_ref(),
        claims.effective_zdr(req.zdr.unwrap_or(false)),
        &claims.org_id,
        crate::zdr_tombstone::Kind::Search,
    );
    let response = search_run(State(state), Extension(claims), Json(req))
        .await
        .into_response();
    if let Some(pending) = pending {
        pending.finish_status(response.status().as_u16());
    }
    response
}

async fn search_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }
    let Some(provider) = &state.search else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "no SERP provider configured".into(),
                code: "UNSUPPORTED".into(),
                hint: Some(
                    "set BRAVE_SEARCH_KEY, SERPER_KEY, or SEARXNG_URL in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    let effective_query = apply_exact_match(&req.query, req.exact_match);
    // A service caller states its posture in the body. Its token's `zdr` claim is a
    // property of its registry row, not of this run (see `Claims::zdr_floor`), so an
    // omitted flag would silently run as non-ZDR. It is refused, never defaulted.
    if claims.is_service() && req.zdr.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "zdr must be sent by a service caller: true or false".into(),
                code: "ZDR_REQUIRED".into(),
                hint: None,
            }),
        )
            .into_response();
    }
    if req
        .score_kinds
        .iter()
        .any(|kind| !SCORE_KINDS.contains(&kind.as_str()))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "score_kinds names a score kind this edge does not produce".into(),
                code: "SCORE_KIND_UNSUPPORTED".into(),
                hint: Some("supported: cross_encoder.v1".into()),
            }),
        )
            .into_response();
    }
    // E3.7.7a: the scrape block is validated only when the flag is on. Off, it is ignored
    // (the response names it `disabled`), so turning the flag off can never turn a search
    // that used to succeed into a 400.
    let scrape_caps = match (&req.scrape, search_scrape_enabled()) {
        (Some(block), true) => match resolve_scrape_block(block) {
            Ok(caps) => Some(caps),
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorBody {
                        error: error.message,
                        code: error.code.into(),
                        hint: None,
                    }),
                )
                    .into_response()
            }
        },
        _ => None,
    };
    // The body flag, raised (never lowered) by the token's ZDR floor for a user of a
    // ZDR org. A service caller's posture is the body flag alone (see above).
    let zdr = claims.effective_zdr(req.zdr.unwrap_or(false));

    // Caller intent hint — forwarded to the router through `SearchOptions`.
    // The rule classifier is still run here, against the *effective* query
    // (post exact-match quoting) because that is the string the router runs its
    // own classifier over, so hint and rules stay comparable on the telemetry.
    let intent_hint_raw = req
        .intent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let intent_hint = intent_hint_raw.and_then(|raw| {
        let parsed = parse_intent_hint(raw);
        if parsed.is_none() {
            // Truncated: `intent` is caller-controlled, and a mis-wired client
            // that stuffs the query into it must not tip user content into
            // logs just because the value failed to parse.
            let shown: String = raw.chars().take(32).collect();
            tracing::warn!(intent = %shown, "search: unrecognized `intent` hint; ignoring");
        }
        parsed
    });
    let intent_hint_label = match (intent_hint_raw, intent_hint) {
        (None, _) => "none",
        (Some(_), Some(intent)) => intent_label(intent),
        (Some(_), None) => "invalid",
    };
    let intent_rule = classify_intent(&effective_query);
    // Resolved here, next to the two values it compares, rather than inline in
    // the event below: the hint is about to be moved into `SearchOptions`, and
    // the rule verdict is only meaningful against the query as it stood before
    // any of that.
    let intent_hint_effect_label =
        intent_hint_effect(intent_hint_raw.is_some(), intent_hint, intent_rule);

    // WS7.E: the web policy decides whether paid providers may run. A policy refusal is a
    // downgrade (the search runs without them and the response names it), never a 403;
    // ZDR stays the floor it always was.
    let decision = match crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        zdr,
        None,
    ) {
        Ok(decision) => decision,
        Err(error) => {
            return (
                StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::FORBIDDEN),
                Json(ErrorBody {
                    error: error.message,
                    code: format!("{:?}", error.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response()
        }
    };
    let paid = crate::web_policy_edge::paid_provider(&decision, req.allow_paid_providers, zdr);
    let policy_downgrades: Vec<String> = paid
        .downgraded_by
        .map(|_| "search.paid_provider".to_string())
        .into_iter()
        .collect();
    let mut opts = build_search_options(&req, &claims.org_id, zdr, intent_hint);
    opts.allow_paid_providers = paid.allowed;

    // ── Cache lookup (org-scoped, intent-driven TTL) ─────────────────────────
    // Key encodes only result-affecting params; highlight/facets/format are
    // re-applied per request so those variants share one entry. The org_id
    // segment is a hard tenant-isolation boundary. Billing + events still fire
    // on a hit (below) — only the expensive provider + synthesis are skipped.
    // The rerank epoch is part of the key: an entry written under one rerank
    // configuration is unreachable under another.
    let rerank_epoch = provider.rerank_epoch();
    let params_sig = params_signature(&opts, req.include_answer, &rerank_epoch);
    let cache_key = crate::cache::SearchCache::key(&claims.org_id, &effective_query, &params_sig);
    // Security review (D2-055): an entry holding an answer is keyed without the web policy, so
    // it may hold an answer synthesized from pages this run's policy refuses. A run under host
    // rules that asks for an answer neither reads nor writes the cache (the scrape family's
    // `is_default` rule); a results-only search is unaffected, its results are not page fetches.
    let answer_cache_ok =
        !req.include_answer || crate::web_policy_edge::is_default(&decision);
    let scache = state
        .redis
        .clone()
        .filter(|_| answer_cache_ok)
        .map(crate::cache::SearchCache::new);
    let from_cache: Option<CachedSearch> = if zdr {
        None
    } else {
        match &scache {
            Some(c) => c.get(&cache_key).await,
            None => None,
        }
    };
    // INNH-8a: an include_answer entry with no answer is a failed synthesis an older build
    // wrote, so it is a miss: the caller asked for an answer and gets a fresh attempt.
    let from_cache = from_cache.filter(|c| usable_hit(c, req.include_answer));

    // E3.7.7b: the scrape block's run, when the include_answer step needed the hydrated pages
    // before the block's own turn below. The block then runs once and the report reads this run.
    let mut early_scrape: Option<ScrapeRun> = None;

    let (results, answer, citations, provider_name, rerank, cache_hit, answer_outcome) = match from_cache {
        Some(c) => {
            let outcome = if req.include_answer {
                AnswerOutcome::from_cached(c.answer.as_deref())
            } else {
                AnswerOutcome::not_asked()
            };
            (c.results, c.answer, c.citations, c.provider, c.rerank, true, outcome)
        }
        None => match provider.search_with_report(&effective_query, &opts).await {
            Ok((results, rerank)) => {
                let provider_name = provider.name().to_string();

                // One `search.upstream` event per upstream call, emitted only
                // on a cache miss — so "attempted" is the count of these
                // events, "failed" the subset with outcome=error, and the
                // cache hit ratio comes off `cache_hit` on the shape event
                // below. `provider` is the configured provider's own static
                // name, so it is bounded by construction.
                tracing::info!(
                    provider = %provider_name,
                    outcome = "ok",
                    hits = results.len(),
                    "search.upstream"
                );

                // include_answer — best-effort synthesis; never fail search on it. INNH-8a: the
                // outcome is named in the response, and ZDR is checked first: under ZDR no page
                // text goes to a model (the model hop has no ZDR posture, ADR-4).
                //
                // E3.7.7b: with a scrape block on the request, the answer reads the pages the block
                // hydrates instead of fetching them again. `capped_pages` is set when one of those
                // pages was cut at the per-result cap: such an answer depends on the caller's cap,
                // which the cache key does not carry, so it is not cached.
                let mut capped_pages = false;
                let (answer, citations, answer_outcome) = if !req.include_answer {
                    (None, None, AnswerOutcome::not_asked())
                } else if zdr {
                    (None, None, AnswerOutcome::skipped_zdr())
                } else {
                    match answer_status::gate(&state) {
                        Err(reason) => (None, None, AnswerOutcome::unavailable(reason)),
                        Ok(p) => {
                            // The block hydrates the results as the response will list them
                            // (host-diversified), so the later report and this run are one run.
                            // No scrape block (or the block disabled) is an empty map: the answer
                            // fetches every page itself, exactly as before.
                            let prefetched = match scrape_caps {
                                Some(caps) => {
                                    let (listed, _demoted) = diversify_results(results.clone());
                                    let run =
                                        run_scrape_block(&state, &decision, zdr, &listed, caps)
                                            .await;
                                    let pages = run.prefetched_pages();
                                    capped_pages = pages
                                        .values()
                                        .any(|page| page.outcome.is_ok() && !page.complete);
                                    early_scrape = Some(run);
                                    pages
                                }
                                None => quarry_runtime::answer::PrefetchedPages::new(),
                            };
                            let areq = AnswerRequest {
                                query: req.query.clone(),
                                top_k: Some(ANSWER_TOP_K),
                                country: req.country.clone(),
                                language: req.language.clone(),
                                zdr: Some(zdr),
                                org_id: Some(claims.org_id.clone()),
                            };
                            // D2-055: the answer's page fetches pass the same web-policy gate
                            // as `/v1/answer`; a denied result URL is never fetched.
                            let gate = crate::answer_routes::gate_for(decision.clone());
                            match p.answer_with_pages(areq, Some(&gate), &prefetched).await {
                                Ok(ar) => {
                                    // Counts only: which pages the answer read from the scrape
                                    // block and which it fetched itself.
                                    tracing::info!(
                                        pages_reused = ar.pages_reused,
                                        pages_fetched = ar.pages_fetched,
                                        "search.answer.pages"
                                    );
                                    let outcome = AnswerOutcome::from_result(&ar);
                                    (Some(ar.answer), Some(ar.citations), outcome)
                                }
                                Err(e) => {
                                    let reason = answer_status::classify_failure(&e);
                                    tracing::warn!(
                                        error_class = crate::zdr_log::class(&e),
                                        reason = reason.as_str(),
                                        "include_answer synthesis failed"
                                    );
                                    (None, None, AnswerOutcome::unavailable(reason))
                                }
                            }
                        }
                    }
                };

                // Write-back (best-effort). Cache failures never fail the request. Only final
                // outcomes are stored, see `cacheable_search`. INNH-8a: a request that asked for
                // an answer and has none (synthesis off, refused or failed) writes nothing, so
                // turning the pipeline on is seen by the very next query and not after the TTL.
                if cacheable_search(zdr, &rerank)
                    && cacheable(zdr, req.include_answer, answer.as_deref())
                    && !capped_pages
                {
                    if let Some(c) = &scache {
                        let ttl = crate::cache::search_ttl_secs(
                            req.topic.as_deref(),
                            opts.time_range.as_deref(),
                            SEARCH_CACHE_DEFAULT_TTL_SECS,
                        );
                        let payload = CachedSearch {
                            provider: provider_name.clone(),
                            results: results.clone(),
                            answer: answer.clone(),
                            citations: citations.clone(),
                            rerank: rerank.clone(),
                        };
                        if let Err(e) = c.put_with_ttl(&cache_key, &payload, ttl).await {
                            // zdr-log-ok: inside the `if !zdr` write-back, so only non-ZDR work reaches this line; the redis error text is what an operator needs
                            tracing::warn!(error = %e, "search cache: put failed");
                        }
                    }
                }

                (results, answer, citations, provider_name, rerank, false, answer_outcome)
            }
            Err(e) => {
                // Emitted before the early returns below, because a chain that
                // is permanently blocked upstream produces no results and no
                // shape event — this is the only place it is visible. The
                // error class is `ErrorCode`, a closed enum, so it is safe as
                // a grouping dimension; `e.message` is not (it carries
                // provider-supplied text) and stays out.
                tracing::warn!(
                    provider = provider.name(),
                    outcome = "error",
                    error_class = ?e.code,
                    "search.upstream"
                );
                let status = match e.code.http_status() {
                    400 => StatusCode::BAD_REQUEST,
                    401 => StatusCode::UNAUTHORIZED,
                    403 => StatusCode::FORBIDDEN,
                    429 => StatusCode::TOO_MANY_REQUESTS,
                    502 => StatusCode::BAD_GATEWAY,
                    504 => StatusCode::GATEWAY_TIMEOUT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                // 3D — structured rate-limit envelope (Tavily-style) on 429.
                if status == StatusCode::TOO_MANY_REQUESTS {
                    return (
                        status,
                        Json(crate::api_error::ApiError::rate_limited(e.message, 60)),
                    )
                        .into_response();
                }
                return (
                    status,
                    Json(ErrorBody {
                        error: e.message,
                        code: format!("{:?}", e.code).to_uppercase(),
                        hint: None,
                    }),
                )
                    .into_response();
            }
        },
    };

    // Result diversity: demote host crowding so one domain can't monopolise the
    // visible results (applied outermost — after the provider merge + any rerank).
    let (mut results, demoted_by_host_cap) = diversify_results(results);
    let count = results.len();

    // The score policy (see `apply_score_policy`). The rerank outcome is logged for each
    // response that has one: its outcome and reason, never the query.
    let score_kind = apply_score_policy(&mut results, &rerank, &req.score_kinds);
    if !rerank.is_not_configured() {
        tracing::info!(
            outcome = rerank.status.label(),
            reason = rerank.reason.as_deref().unwrap_or("none"),
            profile = rerank.profile.as_deref().unwrap_or("none"),
            score_kind = score_kind.unwrap_or("none"),
            cache_hit = cache_hit,
            zdr = zdr,
            hits = count,
            "search.rerank"
        );
    }

    // Source-quality telemetry. This is the shape of what the caller actually
    // got, per engine — the signal the CAPTCHA/rate-limit audit had to curl the
    // container by hand to discover. Emitted before highlight markup is applied
    // so snippet lengths measure the upstream's text, not our `<mark>` tags.
    {
        let mut engine_mix: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut unique_urls: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for result in &results {
            *engine_mix
                .entry(search_engine_label(&result.provider))
                .or_insert(0) += 1;
            unique_urls.insert(result.url.as_str());
        }
        let snippets = snippet_stats(results.iter().map(|r| r.snippet.as_deref()));
        for (engine, hits) in &engine_mix {
            tracing::info!(
                engine = *engine,
                hits = *hits,
                cache_hit = cache_hit,
                "search.engine.contribution"
            );
        }
        tracing::info!(
            provider = %provider_name,
            cache_hit = cache_hit,
            hits_total = count,
            // Divergence from `hits_total` means the router's merge missed a
            // duplicate (differing URL normalization between two engines),
            // which is worth an alert on its own.
            hits_unique_url = unique_urls.len(),
            hits_demoted_host_cap = demoted_by_host_cap,
            engines_contributing = engine_mix.len(),
            engine_mix = %render_engine_mix(&engine_mix),
            snippet_min = snippets.min,
            snippet_median = snippets.median,
            snippet_max = snippets.max,
            snippet_missing = snippets.missing,
            intent_hint = intent_hint_label,
            // What the hint did, not merely that it arrived: the router routes
            // on a supplied hint in place of its own classification, so the
            // countable signal is whether the caller agreed with the rule
            // verdict beside it or displaced it. See `intent_hint_effect`.
            intent_hint_effect = intent_hint_effect_label,
            intent_rule = intent_label(intent_rule),
            "search.result_shape"
        );
    }

    // E3.7.7a: hydrate the top results. Runs after the cache step on every request (hit or
    // miss), so page text is never written to the search cache, under ZDR or not; ZDR stays
    // inline-only by construction. Each URL passes the request's web policy and the policy
    // security engine (preflight) before its fetch; a refusal is a typed per-result failure.
    let scrape_report = match (&req.scrape, scrape_caps) {
        (None, _) => None,
        (Some(_), None) => Some(SearchScrapeReport::disabled()),
        (Some(_), Some(caps)) => {
            // E3.7.7b: the include_answer step may already have run the block over these same
            // results (host-diversified, in the same order); then its run is the report, and no
            // page is fetched a second time.
            let run = match early_scrape.take() {
                Some(run) => run,
                None => run_scrape_block(&state, &decision, zdr, &results, caps).await,
            };
            let report = run.into_report();
            // Counts and closed codes only: no URL, no page text.
            tracing::info!(
                requested = report.requested,
                hydrated = report.hydrated,
                failed = report.failed,
                fetched = report.fetched_pages(),
                zdr = zdr,
                "search.scrape"
            );
            Some(report)
        }
    };

    // format=context — token-bounded RAG context for Verevon.
    let context = match req.format.as_deref() {
        Some("context") => Some(build_context(
            &results,
            req.chunks_per_source,
            MAX_CONTEXT_CHARS,
        )),
        _ => None,
    };

    // Cycle 19 / cluster #19: emit SearchIssued for autocomplete-core.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    let run_id_str = run_id.to_string();
    let run_id_str_for_answer = run_id_str.clone();
    let idem = format!("search:{}", run_id);
    if !zdr {
        state
            .event_sink
            .emit(
                run_id,
                quarry_core::event::EventType::SearchIssued,
                serde_json::json!({
                    "query": req.query,
                    "provider": provider_name,
                    "result_count": count,
                    "limit": opts.limit,
                    "topic": req.topic,
                    "exact_match": req.exact_match,
                    "org_id": claims.org_id,
                    "user_id": claims.user_id,
                    "cache_hit": cache_hit,
                }),
                idem,
            )
            .await;
    }

    // P3 / billing — one unit per query (cache hits included: the query still
    // happened; only upstream compute was saved).
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id_str,
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            1.0,
            serde_json::json!({
                "user_id": claims.user_id,
                "result_count": count,
                "provider": provider_name,
                "query_chars": req.query.chars().count(),
                "include_answer": req.include_answer,
                "answer_status": answer_outcome.status,
                "format": req.format,
                "cache_hit": cache_hit,
                "zdr": zdr,
            }),
        ))
        .await;

    // INNH-8a: include_answer synthesis is metered like /v1/answer, once per model call. Never on
    // a cache hit, a refused or failed synthesis, or a ZDR request (none of those called a model).
    if let Some(synth) = &answer_outcome.metered {
        state
            .usage
            .meter(quarry_runtime::UsageEvent::new(
                format!("answer-{run_id_str_for_answer}"),
                claims.org_id.clone(),
                quarry_runtime::usage_metrics::ANSWER_SYNTH,
                1.0,
                serde_json::json!({
                    "user_id": claims.user_id,
                    "sources_used": synth.sources_used,
                    "sources_skipped": synth.sources_skipped,
                    "model": synth.model,
                    "latency_ms": synth.latency_ms,
                    // E3.7.7b: pages_reused were fetched by the scrape block and are metered
                    // there (search.scrape); this event does not count them again.
                    "pages_fetched": synth.pages_fetched,
                    "pages_reused": synth.pages_reused,
                    "via": "search.include_answer",
                }),
            ))
            .await;
    }

    // E3.7.7a: hydration is metered once per request, by the fetch units that left the edge
    // (a URL refused by the policy or the preflight was never fetched and is not billed). The
    // estimate comes from the job pricing helper; its rates are placeholders (X-12, L4).
    if let Some(report) = &scrape_report {
        let fetched = report.fetched_pages();
        if fetched > 0 {
            let price = scrape_price(fetched);
            state
                .usage
                .meter(quarry_runtime::UsageEvent::new(
                    format!("search-scrape-{run_id_str_for_answer}"),
                    claims.org_id.clone(),
                    quarry_runtime::usage_metrics::SCRAPE_PAGE,
                    fetched as f64,
                    serde_json::json!({
                        "user_id": claims.user_id,
                        "via": "search.scrape",
                        "requested": report.requested,
                        "hydrated": report.hydrated,
                        "failed": report.failed,
                        "estimated_micro_usd": price.total_micro_usd,
                        "cost_source": price.source,
                        "price_clamped": price.clamped,
                        "zdr": zdr,
                    }),
                ))
                .await;
        }
    }

    // 2E — facets computed from clean results (before highlight markup).
    let facets = if req.facets {
        Some(compute_host_facets(&results))
    } else {
        None
    };
    // 2E — highlight query terms in titles/snippets when requested.
    let results = if req.highlight {
        results
            .into_iter()
            .map(|mut r| {
                if let Some(t) = r.title.take() {
                    r.title = Some(highlight_terms(&t, &req.query));
                }
                if let Some(s) = r.snippet.take() {
                    r.snippet = Some(highlight_terms(&s, &req.query));
                }
                r
            })
            .collect()
    } else {
        results
    };

    (
        StatusCode::OK,
        Json(SearchResponse {
            query: req.query,
            provider: provider_name,
            results,
            count,
            answer,
            answer_status: answer_outcome.status,
            answer_reason: answer_outcome.reason,
            citations,
            context,
            facets,
            policy_downgrades,
            score_kind,
            rerank,
            scrape: scrape_report,
        }),
    )
        .into_response()
}

/// INNH-8a: a cache entry is a hit unless the request asked for an answer and the entry has none.
/// Such an entry is a failed synthesis an older build wrote; the caller gets a fresh attempt.
fn usable_hit(entry: &CachedSearch, include_answer: bool) -> bool {
    !include_answer || entry.answer.is_some()
}

/// INNH-8a: whether a finished search may be written to the cache. Never under ZDR, and never when
/// the request asked for an answer and has none (synthesis off, refused or failed): turning the
/// pipeline on must show on the very next query, not after the TTL. An empty answer is a real
/// outcome (no source had text) and is cached.
fn cacheable(zdr: bool, include_answer: bool, answer: Option<&str>) -> bool {
    !zdr && (!include_answer || answer.is_some())
}

/// What the `include_answer` step of one search came to (INNH-8a). `metered` is set only when a
/// model was actually called in this request.
struct AnswerOutcome {
    status: Option<AnswerStatus>,
    reason: Option<AnswerReason>,
    metered: Option<SynthUsage>,
}

struct SynthUsage {
    sources_used: usize,
    sources_skipped: usize,
    model: String,
    latency_ms: u64,
    /// Pages the answer step fetched itself. Pages it read from the scrape block are in
    /// `pages_reused`: their fetch is metered once, by the scrape metering, not here.
    pages_fetched: usize,
    pages_reused: usize,
}

impl AnswerOutcome {
    fn not_asked() -> Self {
        Self { status: None, reason: None, metered: None }
    }

    fn skipped_zdr() -> Self {
        Self { status: Some(AnswerStatus::SkippedZdr), reason: None, metered: None }
    }

    fn unavailable(reason: AnswerReason) -> Self {
        Self { status: Some(AnswerStatus::Unavailable), reason: Some(reason), metered: None }
    }

    /// A pipeline result. No source means no model call was made, so nothing is metered.
    fn from_result(result: &quarry_runtime::answer::AnswerResult) -> Self {
        if result.sources_used == 0 || result.answer.is_empty() {
            return Self { status: Some(AnswerStatus::NoSources), reason: None, metered: None };
        }
        Self {
            status: Some(AnswerStatus::Ok),
            reason: None,
            metered: Some(SynthUsage {
                sources_used: result.sources_used,
                sources_skipped: result.sources_skipped,
                model: result.model.clone(),
                latency_ms: result.latency_ms,
                pages_fetched: result.pages_fetched,
                pages_reused: result.pages_reused,
            }),
        }
    }

    /// A cache hit: the answer was synthesized (and metered) by the request that wrote it.
    fn from_cached(answer: Option<&str>) -> Self {
        match answer {
            Some(a) if !a.is_empty() => Self { status: Some(AnswerStatus::Ok), reason: None, metered: None },
            _ => Self { status: Some(AnswerStatus::NoSources), reason: None, metered: None },
        }
    }
}

// ── /v1/search/images — IMAGES vertical ──────────────────────────────────────

/// Request body for `POST /v1/search/images`. Deliberately minimal: the web
/// `SmartSearchRouter` has no image concept, so this path talks to SearXNG's
/// image vertical directly. Same Bearer auth as `/v1/search`.
#[derive(Debug, Deserialize)]
pub struct ImageSearchRequest {
    pub query: String,
    /// Max images to return. Defaults to 24, capped at 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ImageSearchResponse {
    pub query: String,
    pub provider: String,
    pub images: Vec<ImageResult>,
}

/// `POST /v1/search/images` — focused SearXNG image search.
///
/// Reuses the configured `searxng_url`. When SearXNG isn't configured the
/// route returns 501 with a hint (mirrors `/v1/search`). The response is
/// `{ images: [{ img_src, thumbnail_src, source_url, title }], query,
/// provider: "searxng" }`.
pub async fn images(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ImageSearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(searxng_url) = state.searxng_url.as_deref().filter(|s| !s.is_empty()) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "image search requires a SearXNG provider".into(),
                code: "UNSUPPORTED".into(),
                hint: Some("set SEARXNG_URL (QUARRY_EDGE__SEARXNG_URL) in edge config".into()),
            }),
        )
            .into_response();
    };

    let provider = match SearXNGImages::new(searxng_url) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.message,
                    code: "INTERNAL".into(),
                    hint: None,
                }),
            )
                .into_response();
        }
    };

    let limit = req.limit.unwrap_or(24).clamp(1, 50);
    match provider.search(req.query.trim(), limit).await {
        Ok(images) => {
            meter_vertical_search(&state, &claims, "images", images.len()).await;
            (
                StatusCode::OK,
                Json(ImageSearchResponse {
                    query: req.query,
                    provider: "searxng".into(),
                    images,
                }),
            )
                .into_response()
        }
        Err(e) => vertical_error_response(e),
    }
}

// ── /v1/search/videos — VIDEOS vertical ──────────────────────────────────────

/// Request body for `POST /v1/search/videos`. Mirrors [`ImageSearchRequest`]:
/// the web `SmartSearchRouter` has no video concept, so this path talks to
/// SearXNG's video vertical directly. Same Bearer auth as `/v1/search`.
///
/// This route exists because the BFF was already calling
/// `/api/v1/search/videos`; with no edge route behind it, that call fell
/// through to SearXNG directly and skipped everything the edge is for — org
/// scoping, the usage meter, and the error envelope. A vertical the product
/// ships has to terminate here, even when the handler is thin.
#[derive(Debug, Deserialize)]
pub struct VideoSearchRequest {
    pub query: String,
    /// Max videos to return. Defaults to 24, capped at 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct VideoSearchResponse {
    pub query: String,
    pub provider: String,
    pub videos: Vec<VideoResult>,
}

/// `POST /v1/search/videos` — focused SearXNG video search.
///
/// Reuses the configured `searxng_url`. When SearXNG isn't configured the
/// route returns 501 with a hint (mirrors `/v1/search/images`). The response is
/// `{ videos: [{ url, title, thumbnail_src, iframe_src, author, length,
/// published_date, content }], query, provider: "searxng" }`.
pub async fn videos(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<VideoSearchRequest>,
) -> impl IntoResponse {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "query must not be empty".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(searxng_url) = state.searxng_url.as_deref().filter(|s| !s.is_empty()) else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "video search requires a SearXNG provider".into(),
                code: "UNSUPPORTED".into(),
                hint: Some("set SEARXNG_URL (QUARRY_EDGE__SEARXNG_URL) in edge config".into()),
            }),
        )
            .into_response();
    };

    let provider = match SearXNGVideos::new(searxng_url) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.message,
                    code: "INTERNAL".into(),
                    hint: None,
                }),
            )
                .into_response();
        }
    };

    let limit = req.limit.unwrap_or(24).clamp(1, 50);
    match provider.search(req.query.trim(), limit).await {
        Ok(videos) => {
            meter_vertical_search(&state, &claims, "videos", videos.len()).await;
            (
                StatusCode::OK,
                Json(VideoSearchResponse {
                    query: req.query,
                    provider: "searxng".into(),
                    videos,
                }),
            )
                .into_response()
        }
        Err(e) => vertical_error_response(e),
    }
}

/// Meter one billable unit for a vertical search route (images, videos).
///
/// `/v1/search/images` shipped without this call, so every image search since
/// has been served free while `/v1/search` and `/v1/search/similar` each meter
/// one `SEARCH_QUERY` unit. Nothing in the tree exempts the verticals — no
/// config flag, no comment, no billing-core rule — so the omission reads as an
/// oversight rather than a policy, and copying the images handler for videos
/// would have doubled it. One unit per successful query, matching `similar`,
/// with `kind` naming the vertical so billing can separate them without
/// minting a metric code billing-core does not yet know.
async fn meter_vertical_search(
    state: &AppState,
    claims: &crate::auth::Claims,
    kind: &'static str,
    result_count: usize,
) {
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id.to_string(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            1.0,
            serde_json::json!({
                "user_id": claims.user_id,
                "kind": kind,
                "result_count": result_count,
            }),
        ))
        .await;
}

/// Map a runtime SERP failure onto the vertical routes' error response: the
/// structured rate-limit envelope on 429 (as `/v1/search` does), the plain
/// `ErrorBody` otherwise. Shared by images and videos so the two cannot drift
/// into answering the same upstream failure differently.
fn vertical_error_response(e: quarry_core::error::QuarryError) -> axum::response::Response {
    let status = match e.code.http_status() {
        400 => StatusCode::BAD_REQUEST,
        401 => StatusCode::UNAUTHORIZED,
        403 => StatusCode::FORBIDDEN,
        429 => StatusCode::TOO_MANY_REQUESTS,
        502 => StatusCode::BAD_GATEWAY,
        504 => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    if status == StatusCode::TOO_MANY_REQUESTS {
        return (
            status,
            Json(crate::api_error::ApiError::rate_limited(e.message, 60)),
        )
            .into_response();
    }
    (
        status,
        Json(ErrorBody {
            error: e.message,
            code: format!("{:?}", e.code).to_uppercase(),
            hint: None,
        }),
    )
        .into_response()
}

// ── /v1/search/similar — find-similar (Exa-style) ────────────────────────────

/// Request body for `POST /v1/search/similar`. Supply `text` (a passage to find
/// neighbours for) and/or `url` (its page content is fetched and used as the
/// seed). When both are present, `text` wins.
#[derive(Debug, Deserialize)]
pub struct SimilarRequest {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Max neighbours to return. Defaults to 10, capped at 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Find-similar hits as results, in the Data Plane's order. Quarry does not rank by
/// the Data Plane's per-candidate scores, so a neighbour carries no score.
fn similar_results(hits: Vec<VectorHit>) -> Vec<SearchResult> {
    hits.into_iter()
        .enumerate()
        .map(|(i, h)| SearchResult {
            url: h.url,
            title: h.title,
            snippet: h.snippet,
            rank: (i as u32) + 1,
            provider: "similar".into(),
            score: None,
            ..Default::default()
        })
        .collect()
}

/// Truncate a seed to the embedding budget on a char boundary.
fn truncate_seed(seed: &str) -> String {
    let seed = seed.trim();
    match seed.char_indices().nth(SIMILAR_SEED_MAX_CHARS) {
        Some((idx, _)) => seed[..idx].to_string(),
        None => seed.to_string(),
    }
}

/// `POST /v1/search/similar` — semantic neighbours of a passage or URL.
///
/// Delegates to the Data Plane vector index (Qdrant). Org-scoped via the JWT.
/// Returns 501 when no Data Plane is configured. Results reuse `SearchResponse`
/// (`provider: "similar"`, in the Data Plane's order, with no score).
pub async fn similar(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SimilarRequest>,
) -> impl IntoResponse {
    let text = req.text.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let url = req.url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if text.is_none() && url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "provide `text` or `url`".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let Some(vector) = state.vector_index.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(ErrorBody {
                error: "find-similar requires a Data Plane vector index".into(),
                code: "UNSUPPORTED".into(),
                hint: Some(
                    "set QUARRY_EDGE__DATA_PLANE_URL and QUARRY_EDGE__VECTOR_RETRIEVAL=true in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    // WS7.E: a seed URL is fetched, so the web policy gates it before any fetch. The refusal
    // names the capability and the deciding layer, never the URL.
    if let Some(seed_url) = url {
        let decision = match crate::web_policy_edge::decide(
            &state.policy,
            &claims,
            claims.effective_zdr(false),
            None,
        ) {
            Ok(decision) => decision,
            Err(error) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(ErrorBody {
                        error: error.message,
                        code: "FORBIDDEN".into(),
                        hint: None,
                    }),
                )
                    .into_response();
            }
        };
        if let Err(error) = crate::web_policy_edge::gate_url(&decision, seed_url) {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorBody {
                    error: error.message,
                    code: "FORBIDDEN".into(),
                    hint: None,
                }),
            )
                .into_response();
        }
    }

    // Resolve the seed: explicit text wins; otherwise fetch the URL's content
    // (best-effort — fall back to the URL string itself so the call still runs).
    let (seed, label) = match (text, url) {
        (Some(t), _) => (truncate_seed(t), "text".to_string()),
        (None, Some(u)) => {
            // The request has no `zdr` field, so the posture is the token's
            // floor alone (a user of a ZDR org): see `Claims::zdr_floor`.
            let fetched = SimpleHttpMarkdownFetcher::new()
                .fetch_markdown(u, claims.effective_zdr_mode(None))
                .await
                .map(|md| truncate_seed(&md))
                .filter(|s| !s.is_empty());
            (fetched.unwrap_or_else(|| u.to_string()), u.to_string())
        }
        (None, None) => unreachable!("validated above"),
    };

    let limit = req.limit.unwrap_or(10).clamp(1, 50) as usize;
    // The request has no `zdr` field, so the run's posture is the token's floor alone.
    let query = VectorQuery {
        org_id: &claims.org_id,
        query: &seed,
        zdr: claims.effective_zdr_mode(None),
        limit,
    };
    let (hits, leg_ran) = match vector.retrieve(&query).await {
        Ok(VectorOutcome::Hits(hits)) => (hits, true),
        Ok(VectorOutcome::Skipped(reason)) => {
            // Posture or budget kept the leg from running: there are no neighbours to return.
            tracing::info!(
                marker = reason.marker(),
                "find-similar: the vector leg did not run"
            );
            (Vec::new(), false)
        }
        Err(e) => {
            let status = match e.code.http_status() {
                400 => StatusCode::BAD_REQUEST,
                401 => StatusCode::UNAUTHORIZED,
                403 => StatusCode::FORBIDDEN,
                429 => StatusCode::TOO_MANY_REQUESTS,
                502 => StatusCode::BAD_GATEWAY,
                504 => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (
                status,
                Json(ErrorBody {
                    error: e.message,
                    code: format!("{:?}", e.code).to_uppercase(),
                    hint: None,
                }),
            )
                .into_response();
        }
    };

    let results = similar_results(hits);
    let count = results.len();

    // Meter one unit (same as a search query) for a call that ran the leg. A call the
    // posture or the budget kept from running returned nothing, so it is not billed.
    if leg_ran {
        let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
        state
            .usage
            .meter(quarry_runtime::UsageEvent::new(
                run_id.to_string(),
                claims.org_id.clone(),
                quarry_runtime::usage_metrics::SEARCH_QUERY,
                1.0,
                serde_json::json!({
                    "user_id": claims.user_id,
                    "kind": "similar",
                    "result_count": count,
                }),
            ))
            .await;
    }

    (
        StatusCode::OK,
        Json(SearchResponse {
            query: label,
            provider: "similar".into(),
            results,
            count,
            answer: None,
            answer_status: None,
            answer_reason: None,
            citations: None,
            context: None,
            facets: None,
            policy_downgrades: Vec::new(),
            score_kind: None,
            rerank: RerankReport::default(),
            scrape: None,
        }),
    )
        .into_response()
}

// ── /v1/search/suggest — did-you-mean + related searches (Google-style) ───────

/// Request for `POST /v1/search/suggest`. The SPA fires this in parallel with
/// the main search so suggestions never add latency to results.
#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    pub query: String,
}

/// `corrected_query` is set only when the query likely has a typo; `related`
/// are follow-up searches. Both omitted/empty when the Model Plane is absent or
/// fails — the route always answers 200 so the UI degrades silently.
#[derive(Debug, Serialize, Default)]
pub struct EntityFact {
    pub label: String,
    pub value: String,
}

/// A compact knowledge-panel for the query's primary entity (Google-style),
/// text-only (no image source). Present only when the query is about one
/// specific entity.
#[derive(Debug, Serialize)]
pub struct EntityPanel {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub facts: Vec<EntityFact>,
}

#[derive(Debug, Serialize, Default)]
pub struct SuggestResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected_query: Option<String>,
    pub related_queries: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityPanel>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFact {
    #[serde(default)]
    label: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawEntity {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    facts: Vec<RawFact>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSuggestions {
    #[serde(default)]
    corrected_query: Option<String>,
    #[serde(default, alias = "related")]
    related_queries: Vec<String>,
    #[serde(default)]
    entity: Option<RawEntity>,
}

/// `POST /v1/search/suggest` — Model-Plane-backed spelling correction + related
/// queries. Degrade-safe: returns an empty `200` when `QUARRY_EDGE__SEARCH_SUGGEST` is off
/// (QB-T17), no Model Plane is wired, or the call/parse fails, so the search UX never breaks
/// on suggestions.
pub async fn suggest(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SuggestRequest>,
) -> impl IntoResponse {
    let query = req.query.trim();
    // QB-T17: off unless QUARRY_EDGE__SEARCH_SUGGEST is on and a model path exists.
    if !state.model_plane.search_suggest {
        return (StatusCode::OK, Json(SuggestResponse::default()));
    }
    let Some(mp_url) = state.model_plane_url.as_deref().filter(|s| !s.is_empty()) else {
        return (StatusCode::OK, Json(SuggestResponse::default()));
    };
    if query.is_empty() {
        return (StatusCode::OK, Json(SuggestResponse::default()));
    }
    let suggestions = fetch_suggestions(
        mp_url,
        state.service_token_provider.clone(),
        state.model_plane_token.as_deref(),
        &claims.org_id,
        query,
    )
    .await;
    (StatusCode::OK, Json(suggestions))
}

async fn fetch_suggestions(
    mp_url: &str,
    token_provider: Option<quarry_runtime::service_tokens::SharedServiceTokenProvider>,
    dev_token: Option<&str>,
    org_id: &str,
    query: &str,
) -> SuggestResponse {
    let Ok(mut client) = ModelPlaneClient::new(mp_url) else {
        return SuggestResponse::default();
    };
    if let Some(provider) = token_provider {
        client = client.with_token_provider(provider);
    } else if let Some(t) = dev_token.filter(|t| !t.is_empty()) {
        client = client.with_bearer_token(t);
    }
    let prompt = format!(
        "You are a search assistant. The user searched: \"{query}\".\n\
Return ONLY a JSON object: {{\"corrected_query\": <string or null>, \"related_queries\": [<3-5 strings>], \"entity\": <object or null>}}.\n\
corrected_query: the corrected spelling ONLY if the query likely contains a typo or misspelling; otherwise null (never paraphrase an already-correct query).\n\
related_queries: 3-5 distinct, useful follow-up searches a user might try next. No prose, no numbering.\n\
entity: a compact knowledge panel for the query's PRIMARY entity (one specific person, place, organization, product, or concept) as {{\"name\": string, \"kind\": short type label, \"summary\": 1-2 factual sentences, \"facts\": [{{\"label\": string, \"value\": string}}] with 2-5 key facts}}. Use null when the query is not about one specific entity (e.g. a how-to, comparison, or generic search). Only include facts you are confident are correct."
    );
    let req = ModelPlaneInvokeRequest {
        content: prompt,
        model: None,
        session_key: None,
        thread_id: None,
    };
    match client.invoke_for_org(org_id, &req).await {
        Ok(resp) => parse_suggestions(&resp.content, query),
        Err(e) => {
            tracing::warn!(error_class = crate::zdr_log::class(&e), "suggest: model plane failed; returning no suggestions");
            SuggestResponse::default()
        }
    }
}

/// Parse the model's JSON reply into a clean [`SuggestResponse`]: drop a
/// correction equal to the original query, trim/dedupe related queries, cap at
/// 5. Unparseable replies yield empty (degrade-safe). Pure → unit-tested.
fn parse_suggestions(body: &str, query: &str) -> SuggestResponse {
    let stripped = strip_json_fences(body);
    let raw: RawSuggestions = serde_json::from_str(stripped).unwrap_or_default();
    let corrected = raw
        .corrected_query
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(query));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let related: Vec<String> = raw
        .related_queries
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| {
            !s.is_empty() && !s.eq_ignore_ascii_case(query) && seen.insert(s.to_lowercase())
        })
        .take(5)
        .collect();
    let entity = raw.entity.and_then(|e| {
        let name = e.name.trim().to_owned();
        if name.is_empty() {
            return None;
        }
        let facts: Vec<EntityFact> = e
            .facts
            .into_iter()
            .filter_map(|f| {
                let label = f.label.trim().to_owned();
                let value = f.value.trim().to_owned();
                (!label.is_empty() && !value.is_empty()).then_some(EntityFact { label, value })
            })
            .take(5)
            .collect();
        Some(EntityPanel {
            name,
            kind: e.kind.trim().to_owned(),
            summary: e.summary.trim().to_owned(),
            facts,
        })
    });
    SuggestResponse {
        corrected_query: corrected,
        related_queries: related,
        entity,
    }
}

fn strip_json_fences(body: &str) -> &str {
    let trimmed = body.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim().trim_end_matches("```").trim())
        .unwrap_or(trimmed);
    inner
}

// ── E3.7.7a: search with a scrape block (hydrate results) ─────────────────────
//
// `POST /v1/search` with `"scrape": { ... }` fetches the top results through the edge's
// markdown fetch path (the one `include_answer` and `/v1/answer` use) and returns each page's
// markdown inline, next to the results. Rules:
//
// - Dark: `QUARRY_EDGE__SEARCH_SCRAPE` (default off, read per request). Off, the block is
//   ignored and the response says `scrape.status = "disabled"`.
// - Policy: every URL passes the request's web policy (`gate_url`, D2-055) and then the
//   policy security engine's preflight (blocklists, private hosts, host rules) before it is
//   fetched. A refused URL is never fetched and is a typed per-result failure.
// - Caps: at most `SCRAPE_MAX_LIMIT` results, `SCRAPE_MAX_MAX_CHARS` characters each and one
//   time budget for the whole block; a body above `SCRAPE_MAX_BYTES` is `too_large`.
// - ZDR: hydration runs inline only. Nothing is cached (hydration runs after the cache step,
//   on hits and misses alike), evented or logged; the fetch carries the request's posture.
// - Metering: one `quarry.scrape.page` event per request, quantity = fetch units that left
//   the edge, with the job pricing helper's estimate (placeholder rates) in its metadata.
// - Failure: one closed code per result (`HydrationFailure`); the search itself never fails
//   because a hydration did.
//
// E3.7.7b (runtime `answer.rs`) reuses these hydrated pages for `include_answer`; until then
// a request that sends both fetches the answer's sources separately.

/// The switch for the scrape block on `/v1/search`.
pub const ENV_SEARCH_SCRAPE: &str = "QUARRY_EDGE__SEARCH_SCRAPE";

/// Results hydrated when the block names no `limit`.
const SCRAPE_DEFAULT_LIMIT: u32 = 5;
/// Most results one search may hydrate.
const SCRAPE_MAX_LIMIT: u32 = 10;
/// Characters of markdown kept per result when the block names no `max_chars`.
const SCRAPE_DEFAULT_MAX_CHARS: u32 = 8_000;
/// Most characters of markdown one result may carry.
const SCRAPE_MAX_MAX_CHARS: u32 = 50_000;
/// Time budget for the whole block when it names no `timeout_ms`.
const SCRAPE_DEFAULT_TIMEOUT_MS: u32 = 15_000;
/// Bounds of `timeout_ms` (the same bounds `/v1/scrape` puts on one page).
const SCRAPE_MIN_TIMEOUT_MS: u32 = 1_000;
const SCRAPE_MAX_TIMEOUT_MS: u32 = 55_000;
/// Body cap of one hydrated page.
const SCRAPE_MAX_BYTES: usize = 2_000_000;

/// Whether the scrape block is enabled. Read per request, so a flip needs no restart.
pub fn search_scrape_enabled() -> bool {
    crate::job_results_routes::flag_is_on(std::env::var(ENV_SEARCH_SCRAPE).ok().as_deref())
}

/// The optional `scrape` block of `POST /v1/search`.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct SearchScrapeBlock {
    /// How many of the top results to hydrate, 1 to 10. Default 5.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Characters of markdown kept per result, 1 to 50 000. Default 8 000.
    #[serde(default, alias = "maxChars")]
    pub max_chars: Option<u32>,
    /// Time budget for the whole block in milliseconds, 1 000 to 55 000. Default 15 000.
    #[serde(default, alias = "timeoutMs")]
    pub timeout_ms: Option<u32>,
    /// Output formats. Only `markdown` is supported (and is the default).
    #[serde(default)]
    pub formats: Vec<String>,
}

/// The resolved caps of one scrape block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrapeCaps {
    pub(crate) limit: usize,
    pub(crate) max_chars: usize,
    pub(crate) timeout: std::time::Duration,
}

/// A refused scrape block: a closed code and a message that never echoes caller input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScrapeBlockError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl ScrapeBlockError {
    fn bad(message: &str) -> Self {
        Self {
            code: "BAD_REQUEST",
            message: message.to_string(),
        }
    }
}

/// Validate a scrape block and resolve its defaults. Out-of-range values are refused, not
/// clamped: a caller asking for more than the cap learns it from the 400.
pub(crate) fn resolve_scrape_block(
    block: &SearchScrapeBlock,
) -> Result<ScrapeCaps, ScrapeBlockError> {
    if block.formats.iter().any(|f| f.trim() != "markdown") {
        return Err(ScrapeBlockError {
            code: "UNSUPPORTED_OPTION",
            message: "scrape.formats: only \"markdown\" is supported on /v1/search".into(),
        });
    }
    let limit = block.limit.unwrap_or(SCRAPE_DEFAULT_LIMIT);
    if limit == 0 || limit > SCRAPE_MAX_LIMIT {
        return Err(ScrapeBlockError::bad("scrape.limit must be 1 to 10"));
    }
    let max_chars = block.max_chars.unwrap_or(SCRAPE_DEFAULT_MAX_CHARS);
    if max_chars == 0 || max_chars > SCRAPE_MAX_MAX_CHARS {
        return Err(ScrapeBlockError::bad("scrape.max_chars must be 1 to 50000"));
    }
    let timeout_ms = block.timeout_ms.unwrap_or(SCRAPE_DEFAULT_TIMEOUT_MS);
    if !(SCRAPE_MIN_TIMEOUT_MS..=SCRAPE_MAX_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(ScrapeBlockError::bad(
            "scrape.timeout_ms must be 1000 to 55000",
        ));
    }
    Ok(ScrapeCaps {
        limit: limit as usize,
        max_chars: max_chars as usize,
        timeout: std::time::Duration::from_millis(u64::from(timeout_ms)),
    })
}

/// Why one result was not hydrated. A closed set: never page text, host or provider text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HydrationFailure {
    /// The request's web policy refused the URL. Never fetched.
    PolicyDenied,
    /// The security preflight refused the URL (blocklist, private host, host rule). Never fetched.
    Blocked,
    /// Not an absolute http(s) URL. Never fetched.
    InvalidUrl,
    /// The block's time budget ran out before this page finished.
    DeadlineExceeded,
    /// The server answered, but not with 2xx (`http_status` carries the code).
    HttpStatus,
    /// The body exceeded the per-page byte cap.
    TooLarge,
    /// The body could not be decoded as text.
    Decode,
    /// Extraction failed on a decoded body.
    Extract,
    /// The fetch itself timed out.
    Timeout,
    /// DNS, TLS or connection failure.
    Transport,
    /// Extraction produced nothing.
    Empty,
    /// Extraction produced too little to be the page.
    TooShort,
}

impl HydrationFailure {
    /// The code of a fetcher failure, with the HTTP status when there is one.
    pub(crate) fn from_fetch(failure: &FetchFailure) -> (Self, Option<u16>) {
        match failure {
            FetchFailure::HttpStatus(code) => (Self::HttpStatus, Some(*code)),
            FetchFailure::TooLarge => (Self::TooLarge, None),
            FetchFailure::Decode => (Self::Decode, None),
            FetchFailure::Extract => (Self::Extract, None),
            FetchFailure::Timeout => (Self::Timeout, None),
            FetchFailure::Transport => (Self::Transport, None),
            FetchFailure::Empty => (Self::Empty, None),
            FetchFailure::TooShort => (Self::TooShort, None),
            FetchFailure::PolicyDenied => (Self::PolicyDenied, None),
        }
    }

    /// Whether a request left the edge for this result (a billable fetch unit). Refusals made
    /// before the fetch are not billed.
    pub(crate) fn fetched(self) -> bool {
        !matches!(self, Self::PolicyDenied | Self::Blocked | Self::InvalidUrl)
    }
}

/// The hydration of one result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HydrationOutcome {
    Ok {
        markdown: String,
        /// Characters in `markdown` (after the cap).
        chars: usize,
        /// True when the page had more than `max_chars` characters.
        truncated: bool,
    },
    Failed {
        code: HydrationFailure,
        #[serde(skip_serializing_if = "Option::is_none")]
        http_status: Option<u16>,
    },
}

impl HydrationOutcome {
    fn failed(code: HydrationFailure) -> Self {
        Self::Failed {
            code,
            http_status: None,
        }
    }
}

/// One hydrated result: its URL, its 1-based position in `results`, and the outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HydratedResult {
    pub url: String,
    pub position: usize,
    #[serde(flatten)]
    pub outcome: HydrationOutcome,
}

/// Whether the block ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScrapeStatus {
    /// The block ran; `items` holds one entry per hydrated result.
    Ok,
    /// `QUARRY_EDGE__SEARCH_SCRAPE` is off on this edge; nothing was fetched.
    Disabled,
}

/// The `scrape` field of a search response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchScrapeReport {
    pub status: SearchScrapeStatus,
    /// Results the block tried to hydrate.
    pub requested: usize,
    /// Results that came back with markdown.
    pub hydrated: usize,
    /// Results that came back with a failure code.
    pub failed: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<HydratedResult>,
}

impl SearchScrapeReport {
    pub(crate) fn disabled() -> Self {
        Self {
            status: SearchScrapeStatus::Disabled,
            requested: 0,
            hydrated: 0,
            failed: 0,
            items: Vec::new(),
        }
    }

    pub(crate) fn from_items(items: Vec<HydratedResult>) -> Self {
        let hydrated = items
            .iter()
            .filter(|i| matches!(i.outcome, HydrationOutcome::Ok { .. }))
            .count();
        Self {
            status: SearchScrapeStatus::Ok,
            requested: items.len(),
            hydrated,
            failed: items.len() - hydrated,
            items,
        }
    }

    /// Fetch units that left the edge: hydrated pages plus failures after the fetch began.
    pub(crate) fn fetched_pages(&self) -> usize {
        self.items
            .iter()
            .filter(|i| match &i.outcome {
                HydrationOutcome::Ok { .. } => true,
                HydrationOutcome::Failed { code, .. } => code.fetched(),
            })
            .count()
    }
}

/// The job pricing helper's estimate for `fetched` hydrated pages (static fetch, one attempt,
/// no browser). Placeholder rates: see `quarry_runtime::job_pricing`.
pub(crate) fn scrape_price(fetched: usize) -> JobPrice {
    estimate_job_price(
        JobPriceInput {
            kind: BulkJobKind::SearchHydration,
            pages: u32::try_from(fetched).unwrap_or(u32::MAX),
            browser_pages: 0,
            attempts_per_page: 1,
        },
        JobRates::default(),
    )
}

/// The first `max_chars` characters of `text`, its character count and whether it was cut.
pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> (String, usize, bool) {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => (text[..idx].to_string(), max_chars, true),
        None => (text.to_string(), text.chars().count(), false),
    }
}

/// Hydrate the first `caps.limit` results concurrently under one deadline. The output keeps
/// the order of `results`; every entry has an outcome, so a caller can always tell a refused
/// or failed page from one that was never asked for.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn hydrate_results(
    fetcher: &dyn MarkdownFetcher,
    security: &dyn quarry_security::SecurityEngine,
    gate: &quarry_runtime::answer::UrlGate,
    results: &[SearchResult],
    caps: ScrapeCaps,
    zdr: ZdrMode,
) -> Vec<HydratedResult> {
    hydrate_extractions(fetcher, security, gate, results, caps, zdr)
        .await
        .into_iter()
        .map(|(item, _structured)| item)
        .collect()
}

/// [`hydrate_results`], keeping the structured harvest of each page beside its report entry.
/// The harvest is for the include_answer step that reuses these pages (E3.7.7b): the answer reads
/// the same facts a fetch of its own would have given it. It never reaches the response.
pub(crate) async fn hydrate_extractions(
    fetcher: &dyn MarkdownFetcher,
    security: &dyn quarry_security::SecurityEngine,
    gate: &quarry_runtime::answer::UrlGate,
    results: &[SearchResult],
    caps: ScrapeCaps,
    zdr: ZdrMode,
) -> Vec<(HydratedResult, Option<StructuredData>)> {
    let deadline = tokio::time::Instant::now() + caps.timeout;
    let jobs = results
        .iter()
        .take(caps.limit)
        .enumerate()
        .map(|(i, result)| async move {
            let (outcome, structured) = hydrate_one(
                fetcher,
                security,
                gate,
                &result.url,
                caps.max_chars,
                zdr,
                deadline,
            )
            .await;
            (
                HydratedResult {
                    url: result.url.clone(),
                    position: i + 1,
                    outcome,
                },
                structured,
            )
        });
    futures::future::join_all(jobs).await
}

async fn hydrate_one(
    fetcher: &dyn MarkdownFetcher,
    security: &dyn quarry_security::SecurityEngine,
    gate: &quarry_runtime::answer::UrlGate,
    url: &str,
    max_chars: usize,
    zdr: ZdrMode,
    deadline: tokio::time::Instant,
) -> (HydrationOutcome, Option<StructuredData>) {
    // The web policy first: a denied URL is never parsed further, preflighted or fetched.
    if !gate(url) {
        return (HydrationOutcome::failed(HydrationFailure::PolicyDenied), None);
    }
    let parsed = match url::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => parsed,
        _ => return (HydrationOutcome::failed(HydrationFailure::InvalidUrl), None),
    };
    let run = async {
        if security.preflight(&parsed).await.decision != quarry_security::Decision::Allow {
            return Err(HydrationOutcome::failed(HydrationFailure::Blocked));
        }
        fetcher.fetch_extraction(url, zdr).await.map_err(|failure| {
            let (code, http_status) = HydrationFailure::from_fetch(&failure);
            HydrationOutcome::Failed { code, http_status }
        })
    };
    match tokio::time::timeout_at(deadline, run).await {
        Err(_elapsed) => (HydrationOutcome::failed(HydrationFailure::DeadlineExceeded), None),
        Ok(Err(outcome)) => (outcome, None),
        Ok(Ok(extraction)) => {
            let (markdown, chars, truncated) = truncate_chars(&extraction.markdown, max_chars);
            (
                HydrationOutcome::Ok {
                    markdown,
                    chars,
                    truncated,
                },
                extraction.structured,
            )
        }
    }
}

/// The hydrated pages of one search request, as the scrape block returned them, with the time the
/// block started. In memory for the request only: never cached and never stored.
pub(crate) struct ScrapeRun {
    items: Vec<(HydratedResult, Option<StructuredData>)>,
    fetched_at: chrono::DateTime<chrono::Utc>,
}

impl ScrapeRun {
    /// The report entries, in result order.
    fn into_report(self) -> SearchScrapeReport {
        SearchScrapeReport::from_items(self.items.into_iter().map(|(item, _)| item).collect())
    }

    /// The pages the include_answer step may read instead of fetching again (E3.7.7b), keyed by
    /// result URL.
    ///
    /// - A hydrated page is reused. It is `complete` only when it was not cut at the per-result
    ///   cap, so a capped page feeds the answer but never enters the local corpus.
    /// - A URL the web policy or the security preflight refused is reused as a refusal
    ///   ([`FetchFailure::PolicyDenied`]), so the answer never fetches what the preflight blocked.
    /// - A fetch that failed is reused as that failure, so the same failing page is not fetched twice.
    /// - A page whose hydration ran out of time is left out: the answer fetches it as it always did.
    fn prefetched_pages(&self) -> quarry_runtime::answer::PrefetchedPages {
        use quarry_runtime::answer::{PrefetchedPage, SourceExtraction};
        let mut pages = quarry_runtime::answer::PrefetchedPages::new();
        for (item, structured) in &self.items {
            let (outcome, complete) = match &item.outcome {
                HydrationOutcome::Ok {
                    markdown,
                    truncated,
                    ..
                } => (
                    Ok(SourceExtraction {
                        markdown: markdown.clone(),
                        structured: structured.clone(),
                    }),
                    !*truncated,
                ),
                HydrationOutcome::Failed { code, http_status } => {
                    let failure = match code {
                        HydrationFailure::PolicyDenied
                        | HydrationFailure::Blocked
                        | HydrationFailure::InvalidUrl => FetchFailure::PolicyDenied,
                        HydrationFailure::DeadlineExceeded => continue,
                        HydrationFailure::HttpStatus => match http_status {
                            Some(status) => FetchFailure::HttpStatus(*status),
                            None => continue,
                        },
                        HydrationFailure::TooLarge => FetchFailure::TooLarge,
                        HydrationFailure::Decode => FetchFailure::Decode,
                        HydrationFailure::Extract => FetchFailure::Extract,
                        HydrationFailure::Timeout => FetchFailure::Timeout,
                        HydrationFailure::Transport => FetchFailure::Transport,
                        HydrationFailure::Empty => FetchFailure::Empty,
                        HydrationFailure::TooShort => FetchFailure::TooShort,
                    };
                    (Err(failure), false)
                }
            };
            pages.insert(
                item.url.clone(),
                PrefetchedPage {
                    outcome,
                    complete,
                    fetched_at: self.fetched_at,
                },
            );
        }
        pages
    }
}

/// Run the scrape block over `results`: the first `caps.limit` results, each through the web
/// policy and the security preflight before its fetch. Called once per request; the include_answer
/// step and the response's `scrape` report both read this one run.
async fn run_scrape_block(
    state: &AppState,
    decision: &crate::policy_baseline::Decision,
    zdr: bool,
    results: &[SearchResult],
    caps: ScrapeCaps,
) -> ScrapeRun {
    let gate = crate::answer_routes::gate_for(decision.clone());
    let engine = crate::web_policy_edge::security_engine(&state.security, &state.policy, decision);
    let fetcher = SimpleHttpMarkdownFetcher::new().with_max_bytes(SCRAPE_MAX_BYTES);
    let zdr_mode = if zdr { ZdrMode::On } else { ZdrMode::Off };
    let fetched_at = chrono::Utc::now();
    let items =
        hydrate_extractions(&fetcher, engine.as_ref(), &gate, results, caps, zdr_mode).await;
    ScrapeRun { items, fetched_at }
}

/// QB-T17: the suggestion route's Model Plane switch.
#[cfg(test)]
mod model_plane_switch_tests {
    use super::*;
    use crate::test_support::{claims_for_org, test_state, StubDriver};
    use quarry_runtime::service_tokens::{AuthCoreServiceTokenProvider, SharedServiceTokenProvider};
    use std::sync::Arc;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// With `QUARRY_EDGE__SEARCH_SUGGEST` off, `POST /v1/search/suggest` answers its empty 200
    /// without a mint and without a Model Plane call: the registry refuses that call, so none is made.
    #[tokio::test]
    async fn suggest_makes_no_model_plane_call_while_its_switch_is_off() {
        let auth_core = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "header.payload.signature",
                "expiresInSeconds": 300
            })))
            .mount(&auth_core)
            .await;
        let model_plane = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&model_plane)
            .await;
        let provider: SharedServiceTokenProvider = Arc::new(
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap(),
        );
        let state = AppState {
            model_plane_url: Some(model_plane.uri()),
            service_token_provider: Some(provider),
            ..test_state(StubDriver::ok())
        };

        let response = suggest(
            State(state),
            axum::Extension(claims_for_org("org_suggest")),
            axum::Json(SuggestRequest {
                query: "wire query".into(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body, serde_json::json!({ "related_queries": [] }));
        assert!(
            auth_core.received_requests().await.unwrap().is_empty(),
            "no token is minted while the switch is off"
        );
        assert!(
            model_plane.received_requests().await.unwrap().is_empty(),
            "no Model Plane call is made while the switch is off"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
    use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
    use std::sync::Arc;

    fn r(url: &str, title: &str, snippet: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: Some(title.into()),
            snippet: Some(snippet.into()),
            rank: 1,
            provider: "fake".into(),
            ..Default::default()
        }
    }

    #[test]
    fn truncate_seed_caps_on_char_boundary() {
        let short = "hello world";
        assert_eq!(truncate_seed(short), "hello world");
        let long: String = "é".repeat(SIMILAR_SEED_MAX_CHARS + 100);
        let out = truncate_seed(&long);
        // Capped at the char budget, and still valid UTF-8 (no mid-char split).
        assert_eq!(out.chars().count(), SIMILAR_SEED_MAX_CHARS);
    }

    #[test]
    fn parse_suggestions_drops_echo_and_dedupes() {
        let body = "```json\n{\"corrected_query\":\"rust async\",\"related_queries\":[\"rust async\",\"Rust Async\",\"tokio runtime\",\"async await rust\"]}\n```";
        let out = parse_suggestions(body, "rust asnyc");
        assert_eq!(out.corrected_query.as_deref(), Some("rust async"));
        // "Rust Async" dedupes against "rust async"; cap respected.
        assert_eq!(
            out.related_queries,
            vec![
                "rust async".to_string(),
                "tokio runtime".to_string(),
                "async await rust".to_string(),
            ]
        );
    }

    #[test]
    fn parse_suggestions_drops_correction_equal_to_query() {
        let out = parse_suggestions(
            "{\"corrected_query\":\"rust\",\"related_queries\":[]}",
            "RUST",
        );
        assert!(out.corrected_query.is_none());
    }

    #[test]
    fn parse_suggestions_garbage_is_empty() {
        let out = parse_suggestions("not json at all", "q");
        assert!(out.corrected_query.is_none());
        assert!(out.related_queries.is_empty());
        assert!(out.entity.is_none());
    }

    #[test]
    fn parse_suggestions_builds_entity_panel() {
        let body = r#"{"corrected_query":null,"related_queries":[],"entity":{"name":"Ada Lovelace","kind":"Person","summary":"19th-century mathematician.","facts":[{"label":"Born","value":"1815"},{"label":"","value":"drop"},{"label":"Known for","value":"first algorithm"}]}}"#;
        let entity = parse_suggestions(body, "ada lovelace")
            .entity
            .expect("entity present");
        assert_eq!(entity.name, "Ada Lovelace");
        assert_eq!(entity.kind, "Person");
        // The blank-label fact is dropped; valid ones survive.
        assert_eq!(entity.facts.len(), 2);
        assert_eq!(entity.facts[0].label, "Born");
    }

    #[test]
    fn parse_suggestions_drops_nameless_entity() {
        let body = r#"{"entity":{"name":"  ","summary":"x","facts":[]}}"#;
        assert!(parse_suggestions(body, "q").entity.is_none());
    }

    #[test]
    fn diversify_caps_host_crowding_and_preserves_order() {
        let mk = |url: &str| SearchResult {
            url: url.into(),
            provider: "x".into(),
            ..Default::default()
        };
        let (out, demoted) = diversify_results(vec![
            mk("https://a.com/1"),
            mk("https://a.com/2"),
            mk("https://a.com/3"),
            mk("https://a.com/4"),
            mk("https://b.com/1"),
            mk("https://a.com/5"),
        ]);
        let urls: Vec<&str> = out.iter().map(|r| r.url.as_str()).collect();
        // a.com capped at 3 in the primary block; b.com stays; over-cap a.com trails.
        assert_eq!(
            urls,
            vec![
                "https://a.com/1",
                "https://a.com/2",
                "https://a.com/3",
                "https://b.com/1",
                "https://a.com/4",
                "https://a.com/5",
            ]
        );
        // Demotion is reported for telemetry; nothing is dropped.
        assert_eq!(demoted, 2);
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn diversify_reports_zero_demotions_when_hosts_are_spread() {
        let mk = |url: &str| SearchResult {
            url: url.into(),
            provider: "x".into(),
            ..Default::default()
        };
        let (out, demoted) = diversify_results(vec![mk("https://a.com/1"), mk("https://b.com/1")]);
        assert_eq!(demoted, 0);
        assert_eq!(out.len(), 2);
    }

    fn search_request(value: serde_json::Value) -> SearchRequest {
        serde_json::from_value(value).expect("request body deserializes")
    }

    #[test]
    fn minimal_body_denies_paid_providers() {
        // The permission is absent from every request the fleet sends today,
        // so "absent" is the case that has to be closed.
        let req = search_request(serde_json::json!({ "query": "rust" }));
        assert!(!req.allow_paid_providers);
        assert_eq!(
            req.zdr, None,
            "absent is not sent: the token's floor decides for a user"
        );
    }

    #[test]
    fn non_bool_allow_paid_providers_is_rejected_not_coerced() {
        // `#[serde(default)]` fills in a *missing* field only; a present-but-
        // wrong value must fail the extractor (axum → 422) rather than be read
        // as truthy. `null` is in the list because it is the shape a JS client
        // sends for "unset", and it must not slip past as `false` silently.
        for bad in [
            serde_json::json!("true"),
            serde_json::json!(1),
            serde_json::json!(null),
            serde_json::json!("yes"),
        ] {
            let shown = bad.to_string();
            let mut body = serde_json::Map::new();
            body.insert("query".into(), serde_json::json!("rust"));
            body.insert("allow_paid_providers".into(), bad);
            let parsed = serde_json::from_value::<SearchRequest>(serde_json::Value::Object(body));
            assert!(
                parsed.is_err(),
                "non-bool allow_paid_providers must not deserialize: {shown}"
            );
        }
    }

    #[test]
    fn zdr_forces_paid_providers_closed_even_when_requested() {
        let req = search_request(serde_json::json!({
            "query": "rust",
            "allow_paid_providers": true,
            "zdr": true,
        }));
        let claims = crate::test_support::claims_for_org("org_alpha");
        let opts = build_search_options(
            &req,
            "org_alpha",
            claims.effective_zdr(req.zdr.unwrap_or(false)),
            None,
        );
        assert!(
            !opts.allow_paid_providers,
            "a ZDR request must never carry paid permission to the router"
        );
        assert!(!opts.paid_allowed());
    }

    #[test]
    fn paid_permission_is_granted_when_asked_for_outside_zdr() {
        let req = search_request(serde_json::json!({
            "query": "rust",
            "allow_paid_providers": true,
        }));
        let claims = crate::test_support::claims_for_org("org_alpha");
        let opts = build_search_options(
            &req,
            "org_alpha",
            claims.effective_zdr(req.zdr.unwrap_or(false)),
            None,
        );
        assert!(opts.allow_paid_providers);
        assert!(opts.paid_allowed());
    }

    #[test]
    fn a_users_zdr_floor_closes_paid_providers_when_the_body_says_false() {
        let req = search_request(serde_json::json!({
            "query": "rust",
            "zdr": false,
            "allow_paid_providers": true,
        }));
        let mut claims = crate::test_support::claims_for_org("org_alpha");
        claims.principal_type = Some("user".into());
        claims.zdr = Some(true);
        let opts = build_search_options(
            &req,
            "org_alpha",
            claims.effective_zdr(req.zdr.unwrap_or(false)),
            None,
        );
        assert!(opts.zdr);
        assert!(!opts.allow_paid_providers);
        assert!(!opts.paid_allowed());
    }

    #[test]
    fn a_service_tokens_zdr_claim_does_not_close_paid_providers() {
        // model-gateway / execution-core carry zdr:true by registry row for
        // every org: the run's posture is the body flag.
        let req = search_request(serde_json::json!({
            "query": "rust",
            "zdr": false,
            "allow_paid_providers": true,
        }));
        let mut claims = crate::test_support::claims_for_org("org_alpha");
        claims.principal_type = Some("service".into());
        claims.service_id = Some("service:model-gateway".into());
        claims.zdr = Some(true);
        let opts = build_search_options(
            &req,
            "org_alpha",
            claims.effective_zdr(req.zdr.unwrap_or(false)),
            None,
        );
        assert!(!opts.zdr);
        assert!(opts.allow_paid_providers);
    }

    #[test]
    fn org_scope_comes_from_claims_not_the_body() {
        let req = search_request(serde_json::json!({ "query": "rust", "org_id": "org_evil" }));
        let opts = build_search_options(&req, "org_alpha", false, None);
        assert_eq!(opts.org_id.as_deref(), Some("org_alpha"));
    }

    #[test]
    fn intent_hint_reaches_search_options() {
        let req = search_request(serde_json::json!({ "query": "rust", "intent": "research" }));
        let hint = req.intent.as_deref().and_then(parse_intent_hint);
        let opts = build_search_options(&req, "org_alpha", false, hint);
        assert_eq!(opts.intent, Some(QueryIntent::Research));
    }

    #[test]
    fn intent_participates_in_the_cache_signature() {
        let req = search_request(serde_json::json!({ "query": "rust" }));
        let none = build_search_options(&req, "org_alpha", false, None);
        let research = build_search_options(&req, "org_alpha", false, Some(QueryIntent::Research));
        let navigational =
            build_search_options(&req, "org_alpha", false, Some(QueryIntent::Navigational));
        let epoch = RerankEpoch::default();
        let sig = |opts: &SearchOptions| params_signature(opts, false, &epoch);

        // Two requests that differ only by intent get different engines and so
        // must not be able to read each other's cached results.
        assert_ne!(sig(&research), sig(&navigational));
        assert_ne!(sig(&research), sig(&none));
        // The separation has to survive into the key itself, not just the sig.
        assert_ne!(
            crate::cache::SearchCache::key("org_alpha", "rust", &sig(&research)),
            crate::cache::SearchCache::key("org_alpha", "rust", &sig(&navigational)),
        );
        // ...and identical requests must still collide, or nothing ever hits.
        let research_again =
            build_search_options(&req, "org_alpha", false, Some(QueryIntent::Research));
        assert_eq!(sig(&research), sig(&research_again));
    }

    #[tokio::test]
    async fn videos_route_is_501_when_searxng_is_unconfigured() {
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let response = videos(
            State(state),
            Extension(crate::test_support::claims_for_org("org_alpha")),
            Json(VideoSearchRequest {
                query: "nrk nyheter".into(),
                limit: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["code"], "UNSUPPORTED");
        assert!(body["hint"].as_str().expect("hint").contains("SEARXNG_URL"));
    }

    #[tokio::test]
    async fn videos_route_rejects_a_blank_query() {
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let response = videos(
            State(state),
            Extension(crate::test_support::claims_for_org("org_alpha")),
            Json(VideoSearchRequest {
                query: "   ".into(),
                limit: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn intent_hint_parses_router_vocabulary_and_aliases() {
        assert_eq!(parse_intent_hint("research"), Some(QueryIntent::Research));
        assert_eq!(parse_intent_hint("  NEWS  "), Some(QueryIntent::Fresh));
        assert_eq!(parse_intent_hint("Compare"), Some(QueryIntent::Comparative));
        assert_eq!(parse_intent_hint("general"), Some(QueryIntent::Default));
        assert_eq!(parse_intent_hint("nav"), Some(QueryIntent::Navigational));
    }

    #[test]
    fn intent_hint_unknown_is_dropped_not_guessed() {
        // Must stay `None` rather than falling back to Default: a hint we can't
        // read is a contract mismatch worth surfacing, not a Default request.
        assert!(parse_intent_hint("shopping").is_none());
        assert!(parse_intent_hint("").is_none());
    }

    #[test]
    fn intent_hint_effect_separates_agreement_from_override() {
        // The rules call this Fresh. A caller that says Fresh too agreed; one
        // that says Research displaced them — and because the router overrides
        // rather than seeds, Research is what actually ran. Collapsing the two
        // into "honored" would erase the whole disagreement signal.
        let rule = classify_intent("breaking news today");
        assert_eq!(rule, QueryIntent::Fresh);
        assert_eq!(
            intent_hint_effect(true, Some(QueryIntent::Fresh), rule),
            "used_agreed"
        );
        assert_eq!(
            intent_hint_effect(true, Some(QueryIntent::Research), rule),
            "used_overrode"
        );
    }

    #[test]
    fn intent_hint_effect_reports_the_intent_the_router_routes_on() {
        // Honesty check on the field: the value the label calls "used" is the
        // same one that reaches the router in `SearchOptions::intent`, which
        // `resolve_intent` adopts verbatim instead of classifying.
        let req = search_request(serde_json::json!({
            "query": "breaking news today",
            "intent": "research",
        }));
        let raw = req.intent.as_deref();
        let hint = raw.and_then(parse_intent_hint);
        let opts = build_search_options(&req, "org_alpha", false, hint);
        let rule = classify_intent(&apply_exact_match(&req.query, req.exact_match));

        assert_eq!(opts.intent, Some(QueryIntent::Research));
        assert_ne!(opts.intent, Some(rule));
        assert_eq!(
            intent_hint_effect(raw.is_some(), hint, rule),
            "used_overrode"
        );
    }

    #[test]
    fn unreadable_hint_is_never_reported_as_used() {
        // An unparseable hint is dropped before `SearchOptions`, so the router
        // classified this request itself. Reporting it as used would overstate
        // the caller's reach; reporting it as "none" would bury a mis-wired
        // client in the no-hint baseline.
        let req = search_request(serde_json::json!({ "query": "rust", "intent": "shopping" }));
        let raw = req.intent.as_deref();
        let hint = raw.and_then(parse_intent_hint);
        let opts = build_search_options(&req, "org_alpha", false, hint);
        let rule = classify_intent(&req.query);

        assert_eq!(opts.intent, None);
        assert_eq!(intent_hint_effect(raw.is_some(), hint, rule), "invalid");
        assert_eq!(intent_hint_effect(false, None, rule), "none");
    }

    #[test]
    fn intent_hint_effect_labels_stay_a_closed_set() {
        // The field is a grouping dimension, so every reachable input — valid,
        // absent or malformed — has to land in this fixed set.
        const ALLOWED: [&str; 4] = ["none", "invalid", "used_agreed", "used_overrode"];
        let all = [
            QueryIntent::Navigational,
            QueryIntent::Fresh,
            QueryIntent::Phrase,
            QueryIntent::Research,
            QueryIntent::Comparative,
            QueryIntent::Local,
            QueryIntent::Code,
            QueryIntent::Default,
        ];
        for rule in all {
            for supplied in [true, false] {
                let dropped = intent_hint_effect(supplied, None, rule);
                assert!(ALLOWED.contains(&dropped), "unbounded label: {dropped}");
                for hint in all {
                    let label = intent_hint_effect(supplied, Some(hint), rule);
                    assert!(ALLOWED.contains(&label), "unbounded label: {label}");
                }
            }
        }
    }

    #[test]
    fn intent_labels_are_distinct_and_stable() {
        let all = [
            QueryIntent::Navigational,
            QueryIntent::Fresh,
            QueryIntent::Phrase,
            QueryIntent::Research,
            QueryIntent::Comparative,
            QueryIntent::Local,
            QueryIntent::Code,
            QueryIntent::Default,
        ];
        let labels: std::collections::HashSet<&str> =
            all.iter().map(|i| intent_label(*i)).collect();
        assert_eq!(labels.len(), all.len());
        assert_eq!(intent_label(QueryIntent::Fresh), "fresh");
    }

    #[test]
    fn engine_label_clamps_unknown_to_other() {
        assert_eq!(search_engine_label("searxng"), "searxng");
        assert_eq!(search_engine_label("tantivy_local"), "tantivy_local");
        // Anything an adapter invents stays out of the label space.
        assert_eq!(search_engine_label("searxng-eu-3"), "other");
        assert_eq!(search_engine_label(""), "other");
    }

    #[test]
    fn engine_mix_renders_sorted_and_stable() {
        let mut mix: std::collections::BTreeMap<&'static str, usize> = Default::default();
        mix.insert("searxng", 8);
        mix.insert("brave", 2);
        assert_eq!(render_engine_mix(&mix), "brave=2,searxng=8");
        assert_eq!(render_engine_mix(&Default::default()), "");
    }

    #[test]
    fn snippet_stats_summarise_distribution() {
        let stats = snippet_stats(vec![Some("abcde"), Some("a"), Some("abc")]);
        assert_eq!(
            stats,
            SnippetStats {
                min: 1,
                median: 3,
                max: 5,
                missing: 0,
            }
        );
    }

    #[test]
    fn snippet_stats_count_blank_as_missing_not_zero_length() {
        // A blank snippet must not drag `min` to 0 — a shut-out engine would
        // then look like a terse-but-working one.
        let stats = snippet_stats(vec![Some("abcd"), Some("   "), None]);
        assert_eq!(stats.min, 4);
        assert_eq!(stats.max, 4);
        assert_eq!(stats.missing, 2);
    }

    #[test]
    fn snippet_stats_all_missing_is_zeroed() {
        let stats = snippet_stats(vec![None, None]);
        assert_eq!(
            stats,
            SnippetStats {
                min: 0,
                median: 0,
                max: 0,
                missing: 2,
            }
        );
        assert_eq!(
            snippet_stats(Vec::<Option<&str>>::new()),
            SnippetStats::default()
        );
    }

    #[test]
    fn exact_match_quotes_unquoted() {
        assert_eq!(apply_exact_match("john smith", true), "\"john smith\"");
    }

    #[test]
    fn exact_match_idempotent_on_quoted() {
        assert_eq!(apply_exact_match("\"john smith\"", true), "\"john smith\"");
    }

    #[test]
    fn exact_match_off_passthrough() {
        assert_eq!(apply_exact_match("john smith", false), "john smith");
    }

    #[test]
    fn time_range_explicit_wins() {
        assert_eq!(
            derive_time_range(Some("week"), Some(365)),
            Some("week".into())
        );
    }

    #[test]
    fn time_range_from_days_buckets() {
        assert_eq!(derive_time_range(None, Some(1)), Some("day".into()));
        assert_eq!(derive_time_range(None, Some(5)), Some("week".into()));
        assert_eq!(derive_time_range(None, Some(20)), Some("month".into()));
        assert_eq!(derive_time_range(None, Some(400)), Some("year".into()));
        assert_eq!(derive_time_range(None, None), None);
    }

    #[test]
    fn time_range_invalid_string_falls_to_days() {
        assert_eq!(
            derive_time_range(Some("garbage"), Some(3)),
            Some("week".into())
        );
    }

    #[test]
    fn context_builds_numbered_blocks() {
        let results = vec![
            r("https://a.com/1", "A", "alpha"),
            r("https://b.com/2", "B", "beta"),
        ];
        let ctx = build_context(&results, None, MAX_CONTEXT_CHARS);
        assert!(ctx.contains("[1] A"));
        assert!(ctx.contains("[2] B"));
        assert!(ctx.contains("(https://a.com/1)"));
    }

    #[test]
    fn context_respects_per_source_cap() {
        let results = vec![
            r("https://a.com/1", "A1", "x"),
            r("https://a.com/2", "A2", "y"),
            r("https://a.com/3", "A3", "z"),
        ];
        let ctx = build_context(&results, Some(2), MAX_CONTEXT_CHARS);
        assert!(ctx.contains("A1") && ctx.contains("A2"));
        assert!(!ctx.contains("A3"));
    }

    #[test]
    fn context_respects_char_budget() {
        let results = vec![r("https://a.com/1", "A", &"x".repeat(100))];
        let ctx = build_context(&results, None, 20);
        assert!(ctx.len() <= 20);
    }

    #[test]
    fn host_facets_count_and_sort() {
        let results = vec![
            r("https://a.com/1", "", ""),
            r("https://a.com/2", "", ""),
            r("https://b.com/1", "", ""),
        ];
        let f = compute_host_facets(&results);
        assert_eq!(
            f[0],
            FacetCount {
                value: "a.com".into(),
                count: 2
            }
        );
        assert_eq!(
            f[1],
            FacetCount {
                value: "b.com".into(),
                count: 1
            }
        );
    }

    #[test]
    fn highlight_wraps_terms_case_insensitive() {
        let out = highlight_terms("The Rust Language is rust-y", "rust");
        assert_eq!(out.matches("<mark>").count(), 2);
        assert!(out.contains("<mark>Rust</mark>"));
        assert!(out.contains("<mark>rust</mark>"));
    }

    #[test]
    fn highlight_skips_short_and_handles_quotes() {
        // single-char terms (<2) skipped; quoted phrase unwrapped to tokens.
        let out = highlight_terms("a big cat", "\"a c\"");
        assert!(!out.contains("<mark>")); // "a" and "c" are <2 chars
    }

    struct FakeProvider {
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl SearchProvider for FakeProvider {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.results.clone())
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    struct ErrProvider;

    #[async_trait]
    impl SearchProvider for ErrProvider {
        async fn search(&self, _q: &str, _o: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
            Err(QuarryError::new(ErrorCode::RateLimited, "throttled"))
        }
        fn name(&self) -> &str {
            "err"
        }
    }

    #[tokio::test]
    async fn fake_provider_returns_results() {
        let p: Arc<dyn SearchProvider> = Arc::new(FakeProvider {
            results: vec![r("https://x.com/a", "A", "s")],
        });
        assert_eq!(
            p.search("q", &SearchOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rate_limited_yields_typed_error() {
        let p: Arc<dyn SearchProvider> = Arc::new(ErrProvider);
        let err = p.search("q", &SearchOptions::default()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    // ── structured facts on citations ───────────────────────────────────────

    fn citation_with_figure() -> Citation {
        Citation {
            url: "https://ssb.example/kommunefakta".into(),
            title: Some("Kommunefakta".into()),
            rank: 1,
            provider: "fake".into(),
            outcome: None,
            structured: Some(quarry_transform::structured::StructuredData {
                figures: vec![quarry_transform::structured::KeyFigure {
                    label: "Folketallet".into(),
                    value: "729 437".into(),
                    unit: Some("personer".into()),
                    period: Some("2. kvartal 2026".into()),
                    source: quarry_transform::structured::FigureSource::Hydration,
                }],
                ..Default::default()
            }),
        }
    }

    #[test]
    fn cached_search_round_trips_a_citation_s_structured_facts() {
        // The cache is the one place between the answer pipeline and the
        // client that re-serializes a citation. A field it silently dropped
        // would make the facts appear only on cache misses — the kind of
        // difference nobody notices until a user reports a flaky answer.
        let payload = CachedSearch {
            provider: "fake".into(),
            results: vec![r("https://ssb.example/kommunefakta", "K", "s")],
            answer: Some("Oslo har 729 437 innbyggere.".into()),
            citations: Some(vec![citation_with_figure()]),
            rerank: RerankReport::default(),
        };
        let wire = serde_json::to_string(&payload).unwrap();
        let back: CachedSearch = serde_json::from_str(&wire).unwrap();
        // Bound first: the temporary `Vec` must outlive the borrow below (this
        // test did not compile without it, E0716).
        let citations = back.citations.unwrap();
        let figure = &citations[0]
            .structured
            .as_ref()
            .expect("harvest survives the cache")
            .figures[0];
        assert_eq!(figure.value, "729 437");
        assert_eq!(figure.period.as_deref(), Some("2. kvartal 2026"));
    }

    fn entry(answer: Option<&str>) -> CachedSearch {
        CachedSearch {
            provider: "fake".into(),
            results: Vec::new(),
            answer: answer.map(str::to_owned),
            citations: None,
            rerank: RerankReport::default(),
        }
    }

    #[test]
    fn a_failed_synthesis_is_never_written_to_the_cache() {
        // asked for an answer, has none: not cacheable
        assert!(!cacheable(false, true, None));
        // asked and has one, or has an empty one (no source had text): cacheable
        assert!(cacheable(false, true, Some("Oslo")));
        assert!(cacheable(false, true, Some("")));
        // did not ask: the result set is cacheable with or without an answer
        assert!(cacheable(false, false, None));
        // ZDR never writes
        assert!(!cacheable(true, true, Some("Oslo")));
        assert!(!cacheable(true, false, None));
    }

    #[test]
    fn an_answerless_entry_is_a_miss_only_for_a_request_that_asked_for_an_answer() {
        assert!(!usable_hit(&entry(None), true), "a failed synthesis from an older build");
        assert!(usable_hit(&entry(Some("Oslo")), true));
        assert!(usable_hit(&entry(Some("")), true));
        assert!(usable_hit(&entry(None), false));
    }

    #[test]
    fn the_answer_outcome_names_what_each_path_came_to() {
        let none = AnswerOutcome::not_asked();
        assert!(none.status.is_none() && none.reason.is_none() && none.metered.is_none());
        let zdr = AnswerOutcome::skipped_zdr();
        assert_eq!(zdr.status, Some(AnswerStatus::SkippedZdr));
        assert!(zdr.reason.is_none() && zdr.metered.is_none());
        let off = AnswerOutcome::unavailable(AnswerReason::SynthesisDisabled);
        assert_eq!(off.status, Some(AnswerStatus::Unavailable));
        assert_eq!(off.reason, Some(AnswerReason::SynthesisDisabled));
        assert!(off.metered.is_none());
        let hit = AnswerOutcome::from_cached(Some("Oslo"));
        assert_eq!(hit.status, Some(AnswerStatus::Ok));
        assert!(hit.metered.is_none(), "a cache hit is never metered");
        assert_eq!(
            AnswerOutcome::from_cached(Some("")).status,
            Some(AnswerStatus::NoSources)
        );
    }

    #[test]
    fn the_new_fields_appear_only_when_set() {
        let mut response = SearchResponse {
            query: "oslo".into(),
            provider: "fake".into(),
            results: vec![],
            count: 0,
            answer: None,
            answer_status: None,
            answer_reason: None,
            citations: None,
            context: None,
            facets: None,
            policy_downgrades: Vec::new(),
            score_kind: None,
            rerank: RerankReport::default(),
            scrape: None,
        };
        let plain = serde_json::to_string(&response).unwrap();
        assert!(!plain.contains("answer"), "an unchanged response: {plain}");
        response.answer_status = Some(AnswerStatus::Unavailable);
        response.answer_reason = Some(AnswerReason::NoModelPlaneUrl);
        let wire = serde_json::to_value(&response).unwrap();
        assert_eq!(wire["answer_status"], "unavailable");
        assert_eq!(wire["answer_reason"], "no_model_plane_url");
        assert!(wire.get("answer").is_none());
    }

    #[test]
    fn a_response_without_structured_facts_is_unchanged() {
        let response = SearchResponse {
            query: "oslo".into(),
            provider: "fake".into(),
            results: vec![],
            count: 0,
            answer: Some("…".into()),
            answer_status: None,
            answer_reason: None,
            citations: Some(vec![Citation {
                structured: None,
                ..citation_with_figure()
            }]),
            context: None,
            facets: None,
            policy_downgrades: Vec::new(),
            score_kind: None,
            rerank: RerankReport::default(),
            scrape: None,
        };
        let wire = serde_json::to_string(&response).unwrap();
        assert!(
            !wire.contains("structured"),
            "the new channel must be invisible when a page had none: {wire}"
        );
    }

    #[test]
    fn only_final_search_outcomes_are_written_to_the_cache() {
        let applied = RerankReport::applied(Some("web-v1".into()));
        assert!(cacheable_search(false, &applied));
        assert!(cacheable_search(false, &RerankReport::default()));
        // Transient rerank states are answered live and never replayed.
        for transient in [
            RerankReport::skipped("disabled", None),
            RerankReport::skipped("circuit_open", None),
            RerankReport::degraded("provider_error", None),
        ] {
            assert!(
                !cacheable_search(false, &transient),
                "{transient:?} must not be cached"
            );
        }
        // ZDR never writes, whatever the outcome.
        assert!(!cacheable_search(true, &applied));
        assert!(!cacheable_search(true, &RerankReport::default()));
    }

    #[test]
    fn the_rerank_epoch_is_part_of_the_cache_signature() {
        let req = search_request(serde_json::json!({ "query": "rust" }));
        let opts = build_search_options(&req, "org_alpha", false, None);
        let on = RerankEpoch {
            profile: Some("web-v1".into()),
            enabled: true,
            shadow: false,
            top_n: 10,
        };
        let epochs = [
            RerankEpoch::default(),
            on.clone(),
            RerankEpoch {
                top_n: 20,
                ..on.clone()
            },
            RerankEpoch {
                shadow: true,
                ..on.clone()
            },
            RerankEpoch {
                profile: Some("web-v2".into()),
                ..on.clone()
            },
        ];
        let sigs: Vec<String> = epochs
            .iter()
            .map(|e| params_signature(&opts, false, e))
            .collect();
        for (i, a) in sigs.iter().enumerate() {
            for b in &sigs[i + 1..] {
                assert_ne!(a, b, "two rerank epochs share a signature: {a}");
            }
        }
        // Identical epochs still collide, or nothing ever hits.
        assert_eq!(
            params_signature(&opts, false, &on),
            params_signature(&opts, false, &on.clone()),
        );
    }

    #[test]
    fn paid_permission_participates_in_the_cache_signature() {
        // Paid providers change which results come back, so a request that may
        // not use them must not read an entry that a request which may has written.
        let closed = SearchOptions::default();
        let open = SearchOptions {
            allow_paid_providers: true,
            ..SearchOptions::default()
        };
        let epoch = RerankEpoch::default();
        let sig_closed = params_signature(&closed, false, &epoch);
        let sig_open = params_signature(&open, false, &epoch);
        assert_ne!(sig_closed, sig_open);
    }

    #[test]
    fn cached_entries_written_before_the_report_still_deserialize() {
        // No `rerank` key: the entry reads back as `rerank-not-configured`. Such
        // entries are unreachable once the epoch is in the key, but they must
        // still parse rather than fail the read.
        let old = r#"{"provider":"fake","results":[]}"#;
        let back: CachedSearch = serde_json::from_str(old).unwrap();
        assert_eq!(back.rerank, RerankReport::default());
        assert!(back.answer.is_none());
        assert!(back.citations.is_none());
    }

    #[test]
    fn a_cache_hit_replays_the_report_it_was_written_with() {
        let applied = RerankReport::applied(Some("web-v1".into()));
        let payload = CachedSearch {
            provider: "fake".into(),
            results: vec![r("https://x.com/a", "A", "s")],
            answer: None,
            citations: None,
            rerank: applied.clone(),
        };
        let wire = serde_json::to_string(&payload).unwrap();
        let back: CachedSearch = serde_json::from_str(&wire).unwrap();
        assert_eq!(back.rerank, applied);
    }

    #[test]
    fn similar_hits_keep_the_data_planes_order_and_carry_no_score() {
        let hits = vec![
            VectorHit {
                url: "https://x.test/a".into(),
                title: Some("A".into()),
                snippet: None,
                document_id: None,
            },
            VectorHit {
                url: "https://x.test/b".into(),
                title: None,
                snippet: Some("b".into()),
                document_id: None,
            },
        ];
        let results = similar_results(hits);
        assert_eq!(
            results.iter().map(|r| r.rank).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(results[0].url, "https://x.test/a");
        assert!(results
            .iter()
            .all(|r| r.score.is_none() && r.provider == "similar"));
    }

    #[test]
    fn the_rerank_report_is_absent_from_the_wire_unless_a_rerank_step_was_wired() {
        let mut response = SearchResponse {
            query: "oslo".into(),
            provider: "fake".into(),
            results: vec![],
            count: 0,
            answer: None,
            answer_status: None,
            answer_reason: None,
            citations: None,
            context: None,
            facets: None,
            policy_downgrades: Vec::new(),
            score_kind: None,
            rerank: RerankReport::default(),
            scrape: None,
        };
        let wire = serde_json::to_string(&response).unwrap();
        assert!(
            !wire.contains("rerank"),
            "not configured adds no field: {wire}"
        );
        response.rerank = RerankReport::applied(Some("web-v1".into()));
        let wire = serde_json::to_string(&response).unwrap();
        assert!(
            wire.contains(r#""rerank":{"status":"applied","profile":"web-v1"}"#),
            "{wire}"
        );
    }

    #[test]
    fn a_score_reaches_the_wire_only_for_a_caller_that_asked_for_its_kind() {
        let applied = RerankReport::applied(Some("web-v1".into()));
        let scored = SearchResult {
            score: Some(0.9),
            ..r("https://a.example/", "A", "s")
        };
        let mut asked = vec![scored.clone()];
        let kind = apply_score_policy(&mut asked, &applied, &["cross_encoder.v1".to_owned()]);
        assert_eq!(kind, Some("cross_encoder.v1"));
        assert_eq!(asked[0].score, Some(0.9));

        let mut default = vec![scored];
        let kind = apply_score_policy(&mut default, &applied, &[]);
        assert_eq!(kind, None);
        assert!(
            default.iter().all(|result| result.score.is_none()),
            "the default is the order alone"
        );
    }

    #[test]
    fn the_score_kind_is_named_only_for_an_applied_rerank() {
        let asked = ["cross_encoder.v1".to_owned()];
        for report in [
            RerankReport::skipped("zdr", Some("web-v1".into())),
            RerankReport::degraded("timeout", Some("web-v1".into())),
            RerankReport::shadow_of(&RerankReport::applied(None)),
            RerankReport::default(),
        ] {
            let mut results = vec![SearchResult {
                score: Some(0.5),
                ..r("https://a.example/", "A", "s")
            }];
            assert_eq!(
                apply_score_policy(&mut results, &report, &asked),
                None,
                "{report:?}"
            );
            assert!(
                results.iter().all(|result| result.score.is_none()),
                "a report that did not apply carries no score: {report:?}"
            );
        }
    }

    #[test]
    fn the_request_keeps_zdr_absent_and_reads_the_score_kinds() {
        let absent = search_request(serde_json::json!({ "query": "rust" }));
        assert_eq!(absent.zdr, None);
        assert!(absent.score_kinds.is_empty());
        let asked = search_request(serde_json::json!({
            "query": "rust",
            "zdr": false,
            "score_kinds": ["cross_encoder.v1"],
        }));
        assert_eq!(asked.zdr, Some(false));
        assert_eq!(asked.score_kinds, vec!["cross_encoder.v1".to_owned()]);
    }

    #[test]
    fn the_only_score_kind_the_edge_produces_is_cross_encoder_v1() {
        assert_eq!(SCORE_KINDS, &["cross_encoder.v1"]);
    }
}

#[cfg(test)]
mod search_scrape_tests {
    //! E3.7.7a: the scrape block of `/v1/search`. The flag itself is read from the
    //! environment per request and is not toggled here (parallel tests would race on it);
    //! these tests drive the pieces the handler composes.

    use super::*;
    use async_trait::async_trait;
    use quarry_security::{SecurityEngine, Verdict};
    use std::sync::Arc;
    use std::time::Duration;
    use url::Url;

    struct FakeFetcher;

    #[async_trait]
    impl MarkdownFetcher for FakeFetcher {
        async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String> {
            self.fetch_source(url, zdr).await.ok()
        }

        async fn fetch_source(&self, url: &str, _zdr: ZdrMode) -> Result<String, FetchFailure> {
            if url.contains("ok.test") {
                Ok("hello world".to_string())
            } else if url.contains("long.test") {
                Ok("é".repeat(40))
            } else if url.contains("missing.test") {
                Err(FetchFailure::HttpStatus(404))
            } else if url.contains("slow.test") {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok("too late".to_string())
            } else {
                Err(FetchFailure::Transport)
            }
        }
    }

    /// Blocks `blocked.test`, allows everything else.
    struct FakeSecurity;

    #[async_trait]
    impl SecurityEngine for FakeSecurity {
        async fn preflight(&self, url: &Url) -> Verdict {
            if url.host_str() == Some("blocked.test") {
                Verdict::block("listed")
            } else {
                Verdict::allow()
            }
        }

        async fn check_discovered(&self, _parent: &Url, _child: &Url) -> Verdict {
            Verdict::allow()
        }
    }

    fn result(url: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            provider: "fake".into(),
            ..Default::default()
        }
    }

    fn deny_host(host: &'static str) -> quarry_runtime::answer::UrlGate {
        Arc::new(move |url: &str| !url.contains(host))
    }

    fn caps(limit: usize, max_chars: usize, timeout_ms: u64) -> ScrapeCaps {
        ScrapeCaps {
            limit,
            max_chars,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[test]
    fn an_empty_block_resolves_to_the_defaults() {
        let caps = resolve_scrape_block(&SearchScrapeBlock::default()).unwrap();
        assert_eq!(caps.limit, 5);
        assert_eq!(caps.max_chars, 8_000);
        assert_eq!(caps.timeout, Duration::from_millis(15_000));
    }

    #[test]
    fn out_of_range_caps_are_refused_not_clamped() {
        for block in [
            SearchScrapeBlock { limit: Some(0), ..Default::default() },
            SearchScrapeBlock { limit: Some(11), ..Default::default() },
            SearchScrapeBlock { max_chars: Some(0), ..Default::default() },
            SearchScrapeBlock { max_chars: Some(50_001), ..Default::default() },
            SearchScrapeBlock { timeout_ms: Some(999), ..Default::default() },
            SearchScrapeBlock { timeout_ms: Some(55_001), ..Default::default() },
        ] {
            let err = resolve_scrape_block(&block).unwrap_err();
            assert_eq!(err.code, "BAD_REQUEST");
        }
        let edge = SearchScrapeBlock {
            limit: Some(10),
            max_chars: Some(50_000),
            timeout_ms: Some(55_000),
            formats: vec!["markdown".into()],
        };
        assert!(resolve_scrape_block(&edge).is_ok());
    }

    #[test]
    fn a_format_other_than_markdown_is_an_unsupported_option_and_is_not_echoed() {
        let block = SearchScrapeBlock {
            formats: vec!["<script>screenshot".into()],
            ..Default::default()
        };
        let err = resolve_scrape_block(&block).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_OPTION");
        assert!(!err.message.contains("script"), "caller input is not echoed");
    }

    #[test]
    fn the_request_reads_the_block_with_camel_case_aliases() {
        let req: SearchRequest = serde_json::from_value(serde_json::json!({
            "query": "rust",
            "scrape": { "limit": 3, "maxChars": 100, "timeoutMs": 2000 }
        }))
        .unwrap();
        let block = req.scrape.expect("block parsed");
        assert_eq!(block.limit, Some(3));
        assert_eq!(block.max_chars, Some(100));
        assert_eq!(block.timeout_ms, Some(2_000));
        let plain: SearchRequest =
            serde_json::from_value(serde_json::json!({ "query": "rust" })).unwrap();
        assert!(plain.scrape.is_none());
    }

    #[tokio::test]
    async fn each_result_gets_a_typed_outcome_in_order() {
        let results = vec![
            result("https://ok.test/a"),
            result("https://missing.test/b"),
            result("https://denied.test/c"),
            result("https://blocked.test/d"),
            result("ftp://ok.test/e"),
            result("https://down.test/f"),
            result("https://ok.test/not-asked"),
        ];
        let items = hydrate_results(
            &FakeFetcher,
            &FakeSecurity,
            &deny_host("denied.test"),
            &results,
            caps(6, 100, 2_000),
            ZdrMode::Off,
        )
        .await;
        assert_eq!(items.len(), 6, "the limit caps the hydrated results");
        let positions: Vec<usize> = items.iter().map(|i| i.position).collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            items[0].outcome,
            HydrationOutcome::Ok {
                markdown: "hello world".into(),
                chars: 11,
                truncated: false
            }
        );
        assert_eq!(
            items[1].outcome,
            HydrationOutcome::Failed {
                code: HydrationFailure::HttpStatus,
                http_status: Some(404)
            }
        );
        assert_eq!(items[2].outcome, HydrationOutcome::failed(HydrationFailure::PolicyDenied));
        assert_eq!(items[3].outcome, HydrationOutcome::failed(HydrationFailure::Blocked));
        assert_eq!(items[4].outcome, HydrationOutcome::failed(HydrationFailure::InvalidUrl));
        assert_eq!(items[5].outcome, HydrationOutcome::failed(HydrationFailure::Transport));

        let report = SearchScrapeReport::from_items(items);
        assert_eq!(report.status, SearchScrapeStatus::Ok);
        assert_eq!((report.requested, report.hydrated, report.failed), (6, 1, 5));
        // ok, 404 and transport left the edge; denied, blocked and invalid never did.
        assert_eq!(report.fetched_pages(), 3);
    }

    #[tokio::test]
    async fn markdown_is_cut_at_the_character_cap_on_a_char_boundary() {
        let items = hydrate_results(
            &FakeFetcher,
            &FakeSecurity,
            &deny_host("nothing-denied"),
            &[result("https://long.test/")],
            caps(1, 7, 2_000),
            ZdrMode::On,
        )
        .await;
        assert_eq!(
            items[0].outcome,
            HydrationOutcome::Ok {
                markdown: "é".repeat(7),
                chars: 7,
                truncated: true
            }
        );
    }

    #[tokio::test]
    async fn a_page_past_the_time_budget_is_deadline_exceeded_and_the_rest_still_return() {
        let items = hydrate_results(
            &FakeFetcher,
            &FakeSecurity,
            &deny_host("nothing-denied"),
            &[result("https://slow.test/"), result("https://ok.test/")],
            caps(2, 100, 50),
            ZdrMode::Off,
        )
        .await;
        assert_eq!(
            items[0].outcome,
            HydrationOutcome::failed(HydrationFailure::DeadlineExceeded)
        );
        assert!(matches!(items[1].outcome, HydrationOutcome::Ok { .. }));
        // The slow page's request did leave the edge, so it is billed.
        assert_eq!(SearchScrapeReport::from_items(items).fetched_pages(), 2);
    }

    #[test]
    fn the_wire_shape_names_status_code_and_http_status() {
        let report = SearchScrapeReport::from_items(vec![
            HydratedResult {
                url: "https://ok.test/".into(),
                position: 1,
                outcome: HydrationOutcome::Ok {
                    markdown: "md".into(),
                    chars: 2,
                    truncated: false,
                },
            },
            HydratedResult {
                url: "https://missing.test/".into(),
                position: 2,
                outcome: HydrationOutcome::Failed {
                    code: HydrationFailure::HttpStatus,
                    http_status: Some(404),
                },
            },
            HydratedResult {
                url: "https://denied.test/".into(),
                position: 3,
                outcome: HydrationOutcome::failed(HydrationFailure::PolicyDenied),
            },
        ]);
        let wire = serde_json::to_value(&report).unwrap();
        assert_eq!(wire["status"], "ok");
        assert_eq!(wire["items"][0]["status"], "ok");
        assert_eq!(wire["items"][0]["markdown"], "md");
        assert_eq!(wire["items"][1]["status"], "failed");
        assert_eq!(wire["items"][1]["code"], "http_status");
        assert_eq!(wire["items"][1]["http_status"], 404);
        assert_eq!(wire["items"][2]["code"], "policy_denied");
        assert!(wire["items"][2].get("http_status").is_none());

        let disabled = serde_json::to_value(SearchScrapeReport::disabled()).unwrap();
        assert_eq!(disabled["status"], "disabled");
        assert!(disabled.get("items").is_none());
    }

    #[test]
    fn a_response_without_a_scrape_block_has_no_scrape_field() {
        let response = SearchResponse {
            query: "oslo".into(),
            provider: "fake".into(),
            results: vec![],
            count: 0,
            answer: None,
            answer_status: None,
            answer_reason: None,
            citations: None,
            context: None,
            facets: None,
            policy_downgrades: Vec::new(),
            score_kind: None,
            rerank: RerankReport::default(),
            scrape: None,
        };
        let wire = serde_json::to_string(&response).unwrap();
        assert!(!wire.contains("scrape"), "unchanged response: {wire}");
    }

    #[test]
    fn the_price_uses_the_search_hydration_kind_and_static_fetches_only() {
        let price = scrape_price(4);
        assert_eq!(price.pages_priced, 4);
        assert_eq!(price.browser_pages_priced, 0);
        assert_eq!(price.attempts_priced, 1);
        assert_eq!(price.browser_micro_usd, 0);
        assert_eq!(price.total_micro_usd, price.fetch_micro_usd);
        assert!(!price.clamped);
    }

    #[test]
    fn fetch_failures_map_one_to_one() {
        assert_eq!(
            HydrationFailure::from_fetch(&FetchFailure::HttpStatus(503)),
            (HydrationFailure::HttpStatus, Some(503))
        );
        assert_eq!(
            HydrationFailure::from_fetch(&FetchFailure::PolicyDenied),
            (HydrationFailure::PolicyDenied, None)
        );
        assert!(!HydrationFailure::PolicyDenied.fetched());
        assert!(!HydrationFailure::Blocked.fetched());
        assert!(!HydrationFailure::InvalidUrl.fetched());
        assert!(HydrationFailure::Timeout.fetched());
        assert!(HydrationFailure::DeadlineExceeded.fetched());
    }
}
