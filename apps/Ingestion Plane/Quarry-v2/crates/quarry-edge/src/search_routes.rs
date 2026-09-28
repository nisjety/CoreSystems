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

use quarry_core::zdr::ZdrMode;
use quarry_runtime::answer::{AnswerRequest, Citation, MarkdownFetcher, SimpleHttpMarkdownFetcher};
use quarry_runtime::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use quarry_runtime::serp::{
    ImageResult, SearXNGImages, SearXNGVideos, SearchOptions, SearchResult, VideoResult,
};
use quarry_runtime::smart_router::{classify_intent, QueryIntent};

use crate::state::AppState;

/// Char budget for `format=context` (~2k tokens at ~4 chars/token).
const MAX_CONTEXT_CHARS: usize = 8_000;
/// Cap on sources scraped when `include_answer` is set.
const ANSWER_TOP_K: usize = 5;
/// Max chars of a fetched page used as the find-similar query seed. The whole
/// page would bloat the embedding call; the lead is the strongest signal.
const SIMILAR_SEED_MAX_CHARS: usize = 2_000;

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Zero Data Retention: bypass caches and durable content-bearing events.
    #[serde(default)]
    pub zdr: bool,
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
    /// Optional client-supplied request ID for tracing.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub request_id: Option<String>,
}

fn default_safe_search() -> bool {
    true
}

const fn effective_zdr(requested: bool) -> bool {
    requested
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
}

// ── intent hint + source-quality observability (unit-tested) ────────────────
//
// Everything below emits through `tracing`, not the `metrics` facade. That is
// deliberate: `metrics` and `metrics-exporter-prometheus` are declared in
// Cargo.toml but nothing in this binary ever installs a recorder or serves
// `/metrics`, so `metrics::counter!` would compile to a silent no-op and read
// as instrumented while producing nothing. `tracing` events reach the JSON fmt
// layer and, when `OTEL_EXPORTER_OTLP_ENDPOINT` is set, the OTLP exporter —
// see `telemetry::init_telemetry`.
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
pub(crate) fn params_signature(opts: &SearchOptions, include_answer: bool) -> String {
    format!(
        "l={}|c={:?}|lg={:?}|s={}|t={:?}|tr={:?}|x={}|a={}|inc={:?}|exc={:?}|i={}",
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
    )
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

pub async fn search(
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
    let zdr = effective_zdr(req.zdr);

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

    let opts = build_search_options(&req, &claims.org_id, zdr, intent_hint);

    // ── Cache lookup (org-scoped, intent-driven TTL) ─────────────────────────
    // Key encodes only result-affecting params; highlight/facets/format are
    // re-applied per request so those variants share one entry. The org_id
    // segment is a hard tenant-isolation boundary. Billing + events still fire
    // on a hit (below) — only the expensive provider + synthesis are skipped.
    let params_sig = params_signature(&opts, req.include_answer);
    let cache_key = crate::cache::SearchCache::key(&claims.org_id, &effective_query, &params_sig);
    let scache = state.redis.clone().map(crate::cache::SearchCache::new);
    let from_cache: Option<CachedSearch> = if zdr {
        None
    } else {
        match &scache {
            Some(c) => c.get(&cache_key).await,
            None => None,
        }
    };

    let (results, answer, citations, provider_name, cache_hit) = match from_cache {
        Some(c) => (c.results, c.answer, c.citations, c.provider, true),
        None => match provider.search(&effective_query, &opts).await {
            Ok(results) => {
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

                // include_answer — best-effort synthesis; never fail search on it.
                let (answer, citations) = if req.include_answer {
                    match state.answer_pipeline.as_ref() {
                        Some(p) => {
                            let areq = AnswerRequest {
                                query: req.query.clone(),
                                top_k: Some(ANSWER_TOP_K),
                                country: req.country.clone(),
                                language: req.language.clone(),
                                zdr: Some(zdr),
                                org_id: Some(claims.org_id.clone()),
                            };
                            match p.answer(areq).await {
                                Ok(ar) => (Some(ar.answer), Some(ar.citations)),
                                Err(e) => {
                                    tracing::warn!(error = %e, "include_answer synthesis failed");
                                    (None, None)
                                }
                            }
                        }
                        None => (None, None),
                    }
                } else {
                    (None, None)
                };

                // Write-back (best-effort). Cache failures never fail the request.
                if !zdr {
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
                        };
                        if let Err(e) = c.put_with_ttl(&cache_key, &payload, ttl).await {
                            tracing::warn!(error = %e, "search cache: put failed");
                        }
                    }
                }

                (results, answer, citations, provider_name, false)
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
    let (results, demoted_by_host_cap) = diversify_results(results);
    let count = results.len();

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
                "format": req.format,
                "cache_hit": cache_hit,
                "zdr": zdr,
            }),
        ))
        .await;

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
            citations,
            context,
            facets,
        }),
    )
        .into_response()
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
/// (`provider: "similar"`, each hit's `score` set from the vector distance).
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
                    "set DATA_PLANE_URL (QUARRY_EDGE__DATA_PLANE_URL) in edge config".into(),
                ),
            }),
        )
            .into_response();
    };

    // Resolve the seed: explicit text wins; otherwise fetch the URL's content
    // (best-effort — fall back to the URL string itself so the call still runs).
    let (seed, label) = match (text, url) {
        (Some(t), _) => (truncate_seed(t), "text".to_string()),
        (None, Some(u)) => {
            let fetched = SimpleHttpMarkdownFetcher::new()
                .fetch_markdown(u, ZdrMode::from(false))
                .await
                .map(|md| truncate_seed(&md))
                .filter(|s| !s.is_empty());
            (fetched.unwrap_or_else(|| u.to_string()), u.to_string())
        }
        (None, None) => unreachable!("validated above"),
    };

    let limit = req.limit.unwrap_or(10).clamp(1, 50) as usize;
    let hits = match vector.retrieve(&claims.org_id, &seed, limit).await {
        Ok(h) => h,
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

    let results: Vec<SearchResult> = hits
        .into_iter()
        .enumerate()
        .map(|(i, h)| SearchResult {
            url: h.url,
            title: h.title,
            snippet: h.snippet,
            rank: (i as u32) + 1,
            provider: "similar".into(),
            score: Some(h.score),
            ..Default::default()
        })
        .collect();
    let count = results.len();

    // Meter one unit (same as a search query) so find-similar is billed.
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

    (
        StatusCode::OK,
        Json(SearchResponse {
            query: label,
            provider: "similar".into(),
            results,
            count,
            answer: None,
            citations: None,
            context: None,
            facets: None,
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
/// queries. Degrade-safe: returns an empty `200` when no Model Plane is wired or
/// the call/parse fails, so the search UX never breaks on suggestions.
pub async fn suggest(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<SuggestRequest>,
) -> impl IntoResponse {
    let query = req.query.trim();
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
            tracing::warn!(error = %e, "suggest: model plane failed; returning no suggestions");
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
        assert!(!req.zdr);
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
        let opts = build_search_options(&req, "org_alpha", effective_zdr(req.zdr), None);
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
        let opts = build_search_options(&req, "org_alpha", effective_zdr(req.zdr), None);
        assert!(opts.allow_paid_providers);
        assert!(opts.paid_allowed());
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

        // Two requests that differ only by intent get different engines and so
        // must not be able to read each other's cached results.
        assert_ne!(
            params_signature(&research, false),
            params_signature(&navigational, false)
        );
        assert_ne!(
            params_signature(&research, false),
            params_signature(&none, false)
        );
        // The separation has to survive into the key itself, not just the sig.
        assert_ne!(
            crate::cache::SearchCache::key(
                "org_alpha",
                "rust",
                &params_signature(&research, false)
            ),
            crate::cache::SearchCache::key(
                "org_alpha",
                "rust",
                &params_signature(&navigational, false)
            ),
        );
        // ...and identical requests must still collide, or nothing ever hits.
        assert_eq!(
            params_signature(&research, false),
            params_signature(
                &build_search_options(&req, "org_alpha", false, Some(QueryIntent::Research)),
                false
            )
        );
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
        };
        let wire = serde_json::to_string(&payload).unwrap();
        let back: CachedSearch = serde_json::from_str(&wire).unwrap();
        let figure = &back.citations.unwrap()[0]
            .structured
            .as_ref()
            .expect("harvest survives the cache")
            .figures[0];
        assert_eq!(figure.value, "729 437");
        assert_eq!(figure.period.as_deref(), Some("2. kvartal 2026"));
    }

    #[test]
    fn a_response_without_structured_facts_is_unchanged() {
        let response = SearchResponse {
            query: "oslo".into(),
            provider: "fake".into(),
            results: vec![],
            count: 0,
            answer: Some("…".into()),
            citations: Some(vec![Citation {
                structured: None,
                ..citation_with_figure()
            }]),
            context: None,
            facets: None,
        };
        let wire = serde_json::to_string(&response).unwrap();
        assert!(
            !wire.contains("structured"),
            "the new channel must be invisible when a page had none: {wire}"
        );
    }
}
