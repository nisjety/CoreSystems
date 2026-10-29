//! Deep research: a real multi-round, cited research pipeline that runs inside
//! the gateway.
//!
//! The composer has had a "Dyp research" button since the chat surface shipped,
//! but it was an alias for the Search toggle — it set `browseWeb` and nothing
//! else, so "deep research" was one `web_search` call wearing a different icon.
//! This module is the actual feature.
//!
//! ## Shape
//!
//! ```text
//! plan      one cheap non-streaming inference → 3-6 distinct sub-queries
//! search    every sub-query CONCURRENTLY via web_search, deduped by URL
//! read      the most promising pages CONCURRENTLY via fetch_url, capped
//! synth     one inference over the read passages → report with inline [n]
//! emit      citation per source + the report as a `document` artifact
//! ```
//!
//! It deliberately does NOT go through Temporal. orchestrator-core has research
//! workflows with no production caller; adding a durable workflow here would
//! mean a second unwired path. The pipeline is one turn's work, bounded by a
//! wall clock, and dies with the stream — an in-process pipeline is the honest
//! implementation of that.
//!
//! ## The invariant this module exists to protect
//!
//! **A source that was searched but never successfully read is not evidence.**
//! Quarry returns empty text for JavaScript-rendered pages (see
//! [`crate::tool_loop`]'s `fetch_url` arm), so "found six sources" routinely
//! means "read two". Numbering therefore covers ONLY pages whose text we
//! actually hold: unread sources are still surfaced in the Kilder tab, are
//! listed in the synthesis prompt as explicitly uncitable, and can never
//! receive an `[n]` the report could point at. Everything else here — the caps,
//! the coverage sentence, the mandatory "could not be verified" section — exists
//! so a thin evidence base produces a thin report instead of a confident one.
//!
//! ## Degradation
//!
//! Every phase degrades rather than failing the turn. No Quarry edge, no search
//! results, no readable pages, a failed plan inference, a failed synthesis
//! inference, an exhausted wall clock: each lands in the context message as a
//! stated fact and the turn continues as a normal (honestly hedged) answer. The
//! ONE hard failure is audit persistence, which propagates exactly like the
//! inline tool loop's does — an unaudited tool action is not something to
//! degrade past.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{ChatMessage, InferRequest, ToolCall};
use serde_json::Value;

use crate::{artifacts::ArtifactKind, relevance, sse_events::ChatEvent, state::AppState};

// ---------------------------------------------------------------------------
// Caps. Every one of these is a spend limit, not a tuning knob: deep research
// fans out over the network and then pays for a large-context inference, so an
// unbounded phase is an unbounded bill. Each has an env override clamped to a
// hard ceiling, following `tool_loop::max_tool_rounds`.
// ---------------------------------------------------------------------------

/// Sub-queries the plan may contain.
///
/// Six is the point where an extra sub-query stops adding distinct sources for
/// a normal business question and starts re-finding the same pages under
/// different words — every extra one costs a search round trip plus its share
/// of the corpus budget for a duplicate.
const DEFAULT_MAX_SUB_QUERIES: usize = 6;
/// Absolute ceiling on sub-queries regardless of configuration.
const MAX_SUB_QUERIES_CEILING: usize = 10;
/// One sub-query is still valid research (a narrow question decomposes into
/// itself), so the floor is 1 rather than a "must decompose" rule the model
/// would satisfy by padding.
const MIN_SUB_QUERIES: usize = 1;

/// Results requested per sub-query. Small on purpose: the ranking that matters
/// is cross-sub-query corroboration, and a deep tail from one query is worth
/// less than a shallow head from six.
const SEARCH_RESULTS_PER_SUB_QUERY: i64 = 6;

/// Pages fetched and read.
///
/// Eight pages at [`page_char_cap`] each is ~32k characters — a large but
/// affordable synthesis prompt on every tier we route to. Raising this raises
/// the synthesis input token count linearly, which is the dominant cost of the
/// whole feature.
const DEFAULT_MAX_PAGES: usize = 8;
/// Absolute ceiling on pages fetched regardless of configuration.
const MAX_PAGES_CEILING: usize = 16;
/// Reading nothing is not research; the floor is 1.
const MIN_PAGES: usize = 1;

/// Characters kept from ONE page.
///
/// 4000 matches `tool_loop`'s own `MAX_FETCH_CHARS`, which already truncates
/// every `fetch_url` result upstream of us — so a larger value here would be
/// decorative today. It is still applied on our side so this constant stays
/// authoritative if that one moves, and so one huge page cannot dominate the
/// corpus even when the upstream cap is lifted.
const DEFAULT_PAGE_CHARS: usize = 4_000;
/// Absolute ceiling on per-page extraction regardless of configuration.
const MAX_PAGE_CHARS_CEILING: usize = 20_000;
/// Below ~500 characters a page contributes noise, not evidence.
const MIN_PAGE_CHARS: usize = 500;

/// Total characters of page text admitted into the synthesis prompt.
///
/// The real backstop: pages × per-page is the theoretical worst case, and this
/// is the number that actually bounds the inference bill. 40k characters is
/// roughly 10-12k tokens of evidence, which leaves headroom for the
/// instructions and the report itself inside every context window we route to.
const DEFAULT_CORPUS_CHARS: usize = 40_000;
/// Absolute ceiling on the corpus regardless of configuration.
const MAX_CORPUS_CHARS_CEILING: usize = 120_000;
/// A corpus smaller than one page's worth cannot support a report.
const MIN_CORPUS_CHARS: usize = 1_000;

/// Wall clock for the whole pipeline.
///
/// Deep research is allowed to be slow — the user pressed a button that says so
/// — but not unbounded: a hung upstream must not hold a chat stream open
/// forever. On expiry the pipeline synthesizes from whatever it already holds
/// and says in the report that the budget ran out, which is strictly better
/// than either hanging or discarding real evidence.
const DEFAULT_WALL_CLOCK_SECS: u64 = 120;
/// Absolute ceiling on the wall clock regardless of configuration.
const MAX_WALL_CLOCK_SECS: u64 = 600;
/// Below ~20s not even the plan + one search round completes.
const MIN_WALL_CLOCK_SECS: u64 = 20;

/// Output budget for the synthesis inference (the report itself).
const DEFAULT_REPORT_TOKENS: i32 = 4_096;
/// Absolute ceiling on report output tokens regardless of configuration.
const MAX_REPORT_TOKENS: i32 = 16_384;
/// A report shorter than this cannot carry findings plus an unverified section.
const MIN_REPORT_TOKENS: i32 = 512;

/// Model tier for the planning inference: the Verevon intent layer's cheapest
/// mode. Splitting a question into sub-queries is a rewrite task, not a
/// reasoning task, and it never reaches the user.
///
/// Still a *tier* here, unlike `sse::TITLE_MODEL`, which had to be pinned to a
/// concrete non-reasoning model to make its 24-token budget honest. This one
/// keeps the tier because the budget below is sized for a reasoning model's
/// chain of thought either way, so whichever model the tier resolves to has
/// room to answer.
const PLAN_MODEL: &str = "verevon-budget";
/// Hard ceiling on the planning inference. It gates every later phase, so a
/// slow planner must degrade to "search the question as asked" rather than eat
/// the wall clock the searches need.
///
/// 20s, not 10s: the budget tier resolves to a *reasoning* model, and a
/// measured plan call spent 7.9s (1408 reasoning tokens) before emitting its
/// first visible token. A 10s ceiling made a healthy planner a coin flip.
const PLAN_TIMEOUT: Duration = Duration::from_secs(20);
/// Output budget for 3-6 short sub-queries, one per line.
///
/// Sized for *reasoning* tokens, not just the ~30 tokens of visible output.
/// `verevon-budget` resolves to gpt-5-nano, which bills its chain of thought
/// against this same ceiling: measured live, `max_completion_tokens: 320`
/// returned `finish_reason: "length"` with **320 reasoning tokens and zero
/// content** — an HTTP 200 carrying nothing, which is exactly how deep
/// research came to degrade on every single run. The same call at 2048
/// finished with `stop` after 1408 reasoning tokens.
const PLAN_MAX_TOKENS: i32 = 2048;
/// Hard ceiling on the synthesis inference. Generous (it writes a full report
/// over a large prompt) but finite, so a stalled provider degrades to
/// "corpus in context" rather than hanging the stream.
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(90);

/// Longest sub-query the plan may contain, in characters. A sub-query is a
/// search string; past this the model has written a sentence, and search
/// engines score sentences worse than phrases.
const MAX_SUB_QUERY_CHARS: usize = 200;

/// How often the pipeline checks for cancellation / deadline while a concurrent
/// phase is in flight. 250ms is imperceptible to the user pressing Stop and
/// costs one wakeup per quarter second.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Clamp an env-supplied cap. Pure so the clamping is testable without
/// mutating process env (which would make the test order-dependent).
fn clamped_usize(raw: Option<&str>, default: usize, min: usize, max: usize) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .map_or(default, |parsed| parsed.clamp(min, max))
}

/// Clamp an env-supplied cap expressed in seconds.
fn clamped_u64(raw: Option<&str>, default: u64, min: u64, max: u64) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(default, |parsed| parsed.clamp(min, max))
}

/// Clamp an env-supplied token budget.
fn clamped_i32(raw: Option<&str>, default: i32, min: i32, max: i32) -> i32 {
    raw.and_then(|value| value.trim().parse::<i32>().ok())
        .map_or(default, |parsed| parsed.clamp(min, max))
}

macro_rules! cached_cap {
    ($name:ident, $ty:ty, $clamp:ident, $env:literal, $default:expr, $min:expr, $max:expr, $doc:literal) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name() -> $ty {
            static CACHED: std::sync::OnceLock<$ty> = std::sync::OnceLock::new();
            *CACHED
                .get_or_init(|| $clamp(std::env::var($env).ok().as_deref(), $default, $min, $max))
        }
    };
}

cached_cap!(
    max_sub_queries,
    usize,
    clamped_usize,
    "DEEP_RESEARCH_MAX_SUB_QUERIES",
    DEFAULT_MAX_SUB_QUERIES,
    MIN_SUB_QUERIES,
    MAX_SUB_QUERIES_CEILING,
    "Sub-queries this process will plan and search. Read once — the budget must not change mid-turn."
);
cached_cap!(
    max_pages,
    usize,
    clamped_usize,
    "DEEP_RESEARCH_MAX_PAGES",
    DEFAULT_MAX_PAGES,
    MIN_PAGES,
    MAX_PAGES_CEILING,
    "Pages this process will fetch and read per research turn."
);
cached_cap!(
    page_char_cap,
    usize,
    clamped_usize,
    "DEEP_RESEARCH_PAGE_CHARS",
    DEFAULT_PAGE_CHARS,
    MIN_PAGE_CHARS,
    MAX_PAGE_CHARS_CEILING,
    "Characters kept from one page."
);
cached_cap!(
    corpus_char_cap,
    usize,
    clamped_usize,
    "DEEP_RESEARCH_CORPUS_CHARS",
    DEFAULT_CORPUS_CHARS,
    MIN_CORPUS_CHARS,
    MAX_CORPUS_CHARS_CEILING,
    "Total characters of page text admitted into the synthesis prompt."
);
cached_cap!(
    wall_clock_secs,
    u64,
    clamped_u64,
    "DEEP_RESEARCH_TIMEOUT_SECS",
    DEFAULT_WALL_CLOCK_SECS,
    MIN_WALL_CLOCK_SECS,
    MAX_WALL_CLOCK_SECS,
    "Wall clock for the whole pipeline, in seconds."
);
cached_cap!(
    report_token_budget,
    i32,
    clamped_i32,
    "DEEP_RESEARCH_REPORT_TOKENS",
    DEFAULT_REPORT_TOKENS,
    MIN_REPORT_TOKENS,
    MAX_REPORT_TOKENS,
    "Output tokens for the synthesis inference."
);

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

/// One candidate source discovered by search, plus whatever we managed to read
/// from it.
///
/// `number` is the load-bearing field: `Some(n)` means "we hold this page's own
/// text and the report may cite it as `[n]`", `None` means "search found this
/// but we never read it, so nothing may be attributed to it".
///
/// Not `Eq`: `relevance` is a float, and there is no honest total equality on it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchSource {
    /// Citation number, assigned ONLY once the page's text is in the corpus.
    pub number: Option<usize>,
    pub url: String,
    pub title: String,
    /// The search engine's snippet. Shown in the Kilder tab; never used as
    /// evidence in the report, because a snippet is the engine's summary rather
    /// than the page's own words.
    pub snippet: String,
    /// Indexes of the plan's sub-queries that surfaced this URL. More than one
    /// means independent corroboration, which is the primary read-priority
    /// signal.
    pub sub_queries: Vec<usize>,
    /// Best (lowest) result position this URL reached in any sub-query, 0-based.
    /// Kept as an integer rather than a normalized score because a rank is a
    /// rank — inventing a float from it would look more precise than it is.
    pub best_rank: usize,
    /// Highest reranker score any sub-query's copy of this hit carried, when the
    /// search path supplied one at all. Feeds [`crate::relevance`] as the
    /// preferred signal; `None` means Quarry did not rerank, NOT "scored zero".
    pub provider_score: Option<f32>,
    /// How plausibly this source can answer the question, `0.0`–`1.0`, from
    /// [`crate::relevance::assess`]. Load-bearing twice: it gates whether the
    /// source may be read at all, and it is the primary key of [`read_order`].
    /// `1.0` until the gate has run, so a source is never accidentally
    /// down-ranked by a score nobody computed.
    pub relevance: f32,
    /// True when the relevance gate set this source aside as unable to answer the
    /// question.
    ///
    /// A filtered source is excluded from the read budget and can never receive a
    /// citation number, but it does NOT vanish: it keeps its Kilder row (labelled
    /// via `unread_reason`), it is counted in [`Coverage::found`] and
    /// [`Coverage::filtered`], and it is listed in the synthesis prompt as
    /// explicitly uncitable. Dropping it silently would trade one dishonesty for
    /// another.
    pub filtered: bool,
    /// The page's own text, per-page-capped and corpus-bounded. `None` until the
    /// page is successfully read.
    pub extract: Option<String>,
    /// Why there is no extract, in words the synthesis prompt can show the
    /// model. `None` only when `extract` is `Some`.
    pub unread_reason: Option<String>,
}

impl ResearchSource {
    /// True when this source's own text is available to be cited.
    #[must_use]
    pub fn is_read(&self) -> bool {
        self.number.is_some() && self.extract.is_some()
    }
}

/// What the pipeline actually managed to do, as facts the report must state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Sub-queries the plan contained.
    pub planned: usize,
    /// Sub-queries that returned at least one usable result.
    pub with_results: usize,
    /// Sub-queries whose search call failed outright.
    pub search_failures: usize,
    /// Unique URLs discovered across all sub-queries.
    pub found: usize,
    /// Sources the relevance gate set aside as unable to answer the question.
    /// A subset of `unread`, reported separately so "we found 16 and read 2"
    /// cannot be read as "14 pages failed to load".
    pub filtered: usize,
    /// True when NOTHING cleared the relevance bar and the top-scoring few were
    /// kept anyway. Stated in the coverage line: a silently relaxed filter is the
    /// same dishonesty as a silently strict one.
    pub relevance_fallback: bool,
    /// Sources whose text we hold (and which therefore carry a number).
    pub read: usize,
    /// Sources search found but we could not read.
    pub unread: usize,
    /// Whether the wall clock ran out before the planned work finished.
    pub deadline_hit: bool,
    /// Whether the corpus budget stopped admitting evidence we had fetched.
    pub corpus_exhausted: bool,
}

/// Result of a deep-research turn. Mirrors [`crate::tool_loop::ToolRounds`] so
/// the call site in `sse.rs` treats it the same way, plus the two things only
/// this pipeline produces: a report and a cancellation verdict.
pub struct DeepResearchOutcome {
    /// The conversation with the research context appended.
    pub messages: Vec<ChatMessage>,
    /// Events to emit. Empty when a live sink was supplied — they already
    /// streamed, and replaying them would double every citation.
    pub events: Vec<ChatEvent>,
    /// The concrete model the synthesis resolved to, so the answer can be
    /// produced by the same model (same reasoning as `ToolRounds`).
    pub resolved_model: Option<String>,
    pub any_tool_succeeded: bool,
    pub tool_successes: u32,
    pub tool_failures: u32,
    pub web_citations: u32,
    /// The user pressed Stop. The caller must emit a terminal `stopped` and end
    /// the stream rather than answering.
    pub cancelled: bool,
    /// The synthesized report, when synthesis succeeded.
    pub report: Option<String>,
    /// Coverage facts, for logging and for the caller's own telemetry.
    pub coverage: Coverage,
}

/// Why the pipeline stopped early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    /// The user pressed Stop.
    Cancelled,
    /// The wall clock ran out. NOT a cancellation: we keep the evidence and say
    /// the budget was exhausted.
    Deadline,
}

/// Where a research event goes the moment it happens.
enum ResearchEvents<'a> {
    Live(&'a crate::sse_events::RichEventSink),
    Buffered(Vec<ChatEvent>),
}

impl ResearchEvents<'_> {
    async fn push(&mut self, event: ChatEvent) {
        match self {
            Self::Live(sink) => sink.emit(event).await,
            Self::Buffered(buffer) => buffer.push(event),
        }
    }

    async fn step(&mut self, id: &str, title: &str, detail: &str, status: &str) {
        self.push(ChatEvent::StepUpdate {
            id: id.to_owned(),
            title: title.to_owned(),
            detail: detail.to_owned(),
            status: status.to_owned(),
        })
        .await;
    }

    fn into_buffer(self) -> Vec<ChatEvent> {
        match self {
            Self::Live(_) => Vec::new(),
            Self::Buffered(buffer) => buffer,
        }
    }
}

/// The synthetic tool name the pipeline reports itself under, so the Steps pill
/// and the tool timeline show one `deep_research` action rather than a dozen
/// unexplained `web_search` / `fetch_url` calls.
pub const DEEP_RESEARCH_TOOL: &str = "deep_research";

/// Step ids. Stable strings: the client keys its activity log on them, so a
/// later `status` for the same id updates the row instead of adding one.
const STEP_PLAN: &str = "deep-research-plan";
const STEP_SEARCH: &str = "deep-research-search";
const STEP_READ: &str = "deep-research-read";
const STEP_SYNTHESIZE: &str = "deep-research-report";

// ---------------------------------------------------------------------------
// Phase 1 — plan parsing (pure)
// ---------------------------------------------------------------------------

/// Strip a Markdown code fence, if the model wrapped its answer in one.
fn strip_code_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(after_open) = trimmed.find("```") else {
        return trimmed.to_owned();
    };
    let body = &trimmed[after_open + 3..];
    // The opening fence may carry a language tag on the same line.
    let body = body.split_once('\n').map_or(body, |(first, rest)| {
        if first.trim().is_empty() || first.trim().chars().all(char::is_alphanumeric) {
            rest
        } else {
            body
        }
    });
    body.split("```").next().unwrap_or(body).trim().to_owned()
}

/// Normalize one candidate sub-query: strip list markers and quotes, collapse
/// whitespace, cap length on a char boundary.
fn normalize_sub_query(raw: &str) -> String {
    let mut value = raw.trim();
    // Leading ordinal / bullet: "1.", "2)", "-", "*", "•".
    value = strip_list_marker(value);
    value = value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim();
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_SUB_QUERY_CHARS {
        return collapsed;
    }
    collapsed.chars().take(MAX_SUB_QUERY_CHARS).collect()
}

/// Remove a leading list marker, returning the remainder.
fn strip_list_marker(value: &str) -> &str {
    let trimmed = value.trim_start();
    for marker in ['-', '*', '\u{2022}', '\u{2013}'] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest;
        }
    }
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return trimmed;
    }
    let rest = &trimmed[digits.len()..];
    for separator in [". ", ") ", ".", ")"] {
        if let Some(stripped) = rest.strip_prefix(separator) {
            return stripped;
        }
    }
    trimmed
}

/// True when a candidate could plausibly be a search query.
///
/// Requires at least one alphanumeric character, which is what rejects the
/// structural leftovers of a JSON answer the parser could not read: `{}` and
/// `[]` reach the plain-text path (they parse as JSON, but as neither an array
/// of strings nor a keyed object), and without this filter `{}` becomes the
/// turn's only "sub-query" — six search round trips looking for a brace instead
/// of the fallback to the user's own question.
fn is_plausible_query(candidate: &str) -> bool {
    candidate.chars().any(char::is_alphanumeric)
}

/// True when a line carries a list marker — the signal that separates plan
/// items from the prose a model likes to wrap them in.
fn looks_like_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-')
        || trimmed.starts_with('*')
        || trimmed.starts_with('\u{2022}')
        || trimmed
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
}

/// Pull sub-queries out of a JSON plan: a bare array of strings, an array of
/// `{"query": …}` objects, or an object with a `queries` / `sub_queries` array.
fn parse_json_sub_queries(cleaned: &str) -> Option<Vec<String>> {
    let value: Value = serde_json::from_str(cleaned).ok()?;
    let array = value.as_array().cloned().or_else(|| {
        value.as_object().and_then(|object| {
            ["queries", "sub_queries", "subQueries", "plan"]
                .iter()
                .find_map(|key| object.get(*key))
                .and_then(Value::as_array)
                .cloned()
        })
    })?;
    let items: Vec<String> = array
        .iter()
        .filter_map(|item| {
            item.as_str()
                .map(str::to_owned)
                .or_else(|| item.get("query").and_then(Value::as_str).map(str::to_owned))
        })
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// Pull sub-queries out of a plain-text plan. When the model produced a marked
/// list, take ONLY the marked lines — that is what discards "Here are the
/// sub-queries:" and any trailing commentary. Otherwise every non-empty line is
/// a candidate.
fn parse_list_sub_queries(cleaned: &str) -> Vec<String> {
    let lines: Vec<&str> = cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let marked: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| looks_like_list_item(line))
        .collect();
    let chosen = if marked.len() >= 2 { marked } else { lines };
    chosen
        .into_iter()
        // A line that is only a label ("Sub-queries:") is scaffolding, not a
        // query. Dropping it here rather than in normalization keeps the
        // marked/unmarked decision above honest.
        .filter(|line| !line.ends_with(':'))
        .map(str::to_owned)
        .collect()
}

/// Parse the planning model's answer into distinct sub-queries.
///
/// Never returns an empty plan: a malformed answer degrades to the user's own
/// question. That fallback is the whole point — an empty plan searches nothing,
/// finds nothing, and would let the pipeline emit a confident report over zero
/// evidence.
#[must_use]
pub fn parse_sub_queries(raw: &str, question: &str, max: usize) -> Vec<String> {
    let cleaned = strip_code_fence(raw);
    let candidates =
        parse_json_sub_queries(&cleaned).unwrap_or_else(|| parse_list_sub_queries(&cleaned));

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut plan: Vec<String> = Vec::new();
    for candidate in candidates {
        let normalized = normalize_sub_query(&candidate);
        if normalized.is_empty() || !is_plausible_query(&normalized) {
            continue;
        }
        if !seen.insert(normalized.to_lowercase()) {
            continue;
        }
        plan.push(normalized);
        if plan.len() >= max {
            break;
        }
    }
    if plan.is_empty() {
        let fallback = normalize_sub_query(question);
        if is_plausible_query(&fallback) {
            plan.push(fallback);
        }
    }
    plan
}

/// The planning instruction. Asks for a bare JSON array because that is the
/// shape [`parse_sub_queries`] reads most reliably — but the parser accepts a
/// list too, so a model that ignores the format still produces a usable plan.
fn plan_prompt(question: &str, max: usize) -> String {
    format!(
        "Break this research question into {max} or fewer DISTINCT web-search queries that \
         together cover it. Each query must target a different sub-question, entity, time \
         period, or perspective — never a reworded duplicate. Write them in the language the \
         question is most likely to be documented in (for Norwegian subjects that is usually \
         Norwegian; for international subjects usually English). Each query is a short search \
         phrase, not a sentence, and carries no operators.\n\n\
         Answer with ONLY a JSON array of strings. No prose, no explanation, no code fence.\n\n\
         Question: {question}"
    )
}

// ---------------------------------------------------------------------------
// Phase 2 — dedupe (pure)
// ---------------------------------------------------------------------------

/// A dedupe key for a URL: host + path, scheme-insensitive, `www.`-insensitive,
/// fragment-free, trailing-slash-free.
///
/// The scheme is dropped deliberately. `http://x.no/a` and `https://x.no/a` are
/// the same page, and counting them as two sources would inflate both the
/// corroboration signal and the citation list with one document twice.
#[must_use]
pub fn normalize_url_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let rest = without_fragment
        .split_once("://")
        .map_or(without_fragment, |(_scheme, rest)| rest);
    let (host, path) = rest
        .split_once('/')
        .map_or((rest, String::new()), |(host, path)| {
            (host, format!("/{path}"))
        });
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(host.as_str());
    let path = path.trim_end_matches('/');
    format!("{host}{path}")
}

/// One raw search hit, tagged with the sub-query that produced it and its
/// position in that sub-query's result list.
///
/// Not `Eq`: `score` is a float, and there is no honest total equality on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub sub_query: usize,
    pub rank: usize,
    pub url: String,
    pub title: String,
    pub snippet: String,
    /// Quarry's semantic-reranker relevance for this hit, when the search tool
    /// passed one through. See [`hits_from_search_output`] for why this is
    /// `None` on every deployment today.
    pub score: Option<f32>,
}

/// Collapse hits from every sub-query into unique candidate sources.
///
/// First-seen order is preserved (so the highest-ranked hit of the first
/// sub-query stays first), and every sub-query that surfaced a URL is recorded —
/// that count is the corroboration signal [`read_order`] ranks on.
#[must_use]
pub fn dedupe_hits(hits: &[SearchHit]) -> Vec<ResearchSource> {
    let mut sources: Vec<ResearchSource> = Vec::new();
    let mut index_by_key: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for hit in hits {
        let url = hit.url.trim();
        if url.is_empty() {
            continue;
        }
        let key = normalize_url_key(url);
        if key.is_empty() {
            continue;
        }
        if let Some(&existing) = index_by_key.get(&key) {
            let source = &mut sources[existing];
            if !source.sub_queries.contains(&hit.sub_query) {
                source.sub_queries.push(hit.sub_query);
            }
            source.best_rank = source.best_rank.min(hit.rank);
            // Keep the highest reranker score any sub-query's copy carried. The
            // reranker only scores the head of each response, so the same URL is
            // routinely scored under one sub-query and unscored under another;
            // taking the max means one sub-query's judgement is not lost to
            // another's silence.
            source.provider_score = match (source.provider_score, hit.score) {
                (Some(existing), Some(incoming)) => Some(existing.max(incoming)),
                (existing, incoming) => existing.or(incoming),
            };
            // Keep the richest metadata we have seen for this URL: providers
            // differ on which fields they populate for the same page.
            if source.title.trim().is_empty() && !hit.title.trim().is_empty() {
                hit.title.trim().clone_into(&mut source.title);
            }
            if source.snippet.trim().is_empty() && !hit.snippet.trim().is_empty() {
                hit.snippet.trim().clone_into(&mut source.snippet);
            }
            continue;
        }
        index_by_key.insert(key, sources.len());
        sources.push(ResearchSource {
            number: None,
            url: url.to_owned(),
            // Deliberately left as the provider gave it, even when empty. Using
            // the URL as a placeholder here would make the field non-empty, and
            // the backfill above would then refuse the real title a later
            // duplicate carries — a source whose Kilder row reads as a raw URL
            // when a proper title was available all along.
            title: hit.title.trim().to_owned(),
            snippet: hit.snippet.trim().to_owned(),
            sub_queries: vec![hit.sub_query],
            best_rank: hit.rank,
            provider_score: hit.score,
            // Neutral until `apply_relevance_gate` runs. Defaulting to 0.0 would
            // make an ungated call silently rank every source as irrelevant.
            relevance: 1.0,
            filtered: false,
            extract: None,
            unread_reason: Some(NOT_ATTEMPTED.to_owned()),
        });
    }
    // Resolve the URL fallback once, after every duplicate has had its chance
    // to contribute a real title, so no Kilder row is ever blank.
    for source in &mut sources {
        if source.title.is_empty() {
            source.title = source.url.clone();
        }
    }
    sources
}

/// Stated when a source was never selected for reading, so the synthesis prompt
/// never has an unread source with a blank explanation.
const NOT_ATTEMPTED: &str = "not selected for reading (page budget spent on higher-ranked sources)";

/// How many relevance tiers [`read_order`] sorts on.
///
/// Relevance is bucketed rather than compared as a raw float on purpose. The
/// score is a plausibility estimate, not a measurement: claiming 0.62 beats 0.61
/// would be false precision, and it would also throw away the corroboration
/// signal, which is genuinely informative *between* sources of comparable
/// relevance. Four buckets (`<0.25`, `<0.5`, `<0.75`, `≥0.75`) is the coarsest
/// split that still separates "clearly on topic" from "probably not".
const RELEVANCE_TIERS: usize = 4;

/// Which relevance bucket a score falls in. Higher is better.
// reason: RELEVANCE_TIERS is 4; the product is bounded by 4.0 and never negative
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn relevance_tier(relevance: f32) -> usize {
    let scaled = (relevance.clamp(0.0, 1.0) * RELEVANCE_TIERS as f32) as usize;
    scaled.min(RELEVANCE_TIERS - 1)
}

/// What the relevance gate did, as facts the coverage line must state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateReport {
    /// Sources set aside as unable to answer the question.
    pub filtered: usize,
    /// True when nothing cleared the bar and the top few were kept anyway.
    pub fallback_used: bool,
}

/// Score every source against the question and set aside the ones that cannot
/// plausibly answer it.
///
/// This is the gate that did not exist. Before it, every URL any sub-query
/// returned was a read candidate and, once read, a numbered citation — which is
/// how a Norwegian weather question came to cite an Instagram post about Paris
/// cafés. It runs BEFORE reading (so the page budget is not spent on noise) and
/// therefore before citing (a source that is never read is never numbered).
///
/// A filtered source is not deleted. It keeps its Kilder row, is counted, and is
/// listed in the synthesis prompt as explicitly uncitable, with
/// [`crate::relevance::filtered_reason`] saying why in words. See
/// [`ResearchSource::filtered`].
pub fn apply_relevance_gate(question: &str, sources: &mut [ResearchSource]) -> GateReport {
    if sources.is_empty() {
        return GateReport::default();
    }
    let parsed = relevance::Question::parse(question);
    let verdicts: Vec<relevance::Verdict> = sources
        .iter()
        .map(|source| {
            relevance::assess(
                &parsed,
                &relevance::Candidate {
                    url: &source.url,
                    title: &source.title,
                    snippet: &source.snippet,
                    provider_score: source.provider_score,
                },
            )
        })
        .collect();
    let mask = relevance::keep_mask(&verdicts);

    let mut filtered = 0usize;
    for ((source, verdict), keep) in sources.iter_mut().zip(&verdicts).zip(&mask.keep) {
        source.relevance = verdict.score;
        if *keep {
            // Left otherwise untouched, including `unread_reason`: the read phase
            // owns that field and writes the specific reason a page could not be
            // fetched. When the never-empty fallback fired, that fact is global
            // rather than per-source, and it is carried by
            // `Coverage::relevance_fallback` into the coverage line the report is
            // required to reproduce.
            source.filtered = false;
            continue;
        }
        source.filtered = true;
        source.extract = None;
        source.number = None;
        source.unread_reason = Some(relevance::filtered_reason(verdict));
        filtered = filtered.saturating_add(1);
    }

    GateReport {
        filtered,
        fallback_used: mask.fallback_used,
    }
}

/// Read priority: most relevant tier first, then most corroborated, then best
/// search rank, then discovery order. Filtered sources are excluded outright.
///
/// Relevance leads because corroboration was actively harmful without it: six
/// sub-queries all surfacing the same off-topic page counted as six independent
/// votes for noise, and that page then out-ranked the one source that actually
/// answered the question. Within a relevance tier corroboration is still the
/// right signal — a page two independent sub-queries both surfaced beats the top
/// hit of one narrow phrasing.
///
/// Excluding filtered sources here is what spends the read budget honestly: a
/// source that is never in `order` is never fetched by [`pages_to_read`] and
/// never numbered by [`number_and_bound`].
#[must_use]
pub fn read_order(sources: &[ResearchSource]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..sources.len())
        .filter(|&index| !sources[index].filtered)
        .collect();
    order.sort_by(|&left, &right| {
        let a = &sources[left];
        let b = &sources[right];
        relevance_tier(b.relevance)
            .cmp(&relevance_tier(a.relevance))
            .then(b.sub_queries.len().cmp(&a.sub_queries.len()))
            .then(a.best_rank.cmp(&b.best_rank))
            .then(left.cmp(&right))
    });
    order
}

/// The pages to fetch: read priority, truncated to the page budget.
///
/// Its own function rather than an inline `.take()` so the page cap is
/// *enforced somewhere testable* — the fetch phase is the one that spends real
/// network time and real audit rows, and "we only fetch N" is a claim that
/// should fail a test if it ever stops being true.
#[must_use]
pub fn pages_to_read(order: &[usize], cap: usize) -> Vec<usize> {
    order.iter().copied().take(cap).collect()
}

// ---------------------------------------------------------------------------
// Phase 3/4 — extraction bounds + numbering (pure)
// ---------------------------------------------------------------------------

/// Cap one page's text, saying in words that it was cut.
///
/// A bare ellipsis reads as "that is all the page said", which would let the
/// model summarize a truncated page as complete — the same reasoning as
/// `tool_loop::bounded_tool_output`.
#[must_use]
pub fn bounded_extract(text: &str, cap: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= cap {
        return trimmed.to_owned();
    }
    let kept: String = trimmed.chars().take(cap).collect();
    format!("{kept}\n[truncated at {cap} characters — this page continues beyond what was read]")
}

/// Assign citation numbers and enforce the corpus budget.
///
/// Walks `order` (read priority) and numbers each source whose extract fits in
/// the remaining corpus budget. A source that does not fit loses its extract and
/// gains a stated reason, so it degrades to "found, not read" instead of being
/// silently citable with no text behind it.
///
/// Returns the number of sources dropped by the corpus budget.
pub fn number_and_bound(
    sources: &mut [ResearchSource],
    order: &[usize],
    corpus_cap: usize,
) -> usize {
    let mut used = 0usize;
    let mut next_number = 1usize;
    let mut dropped = 0usize;

    for &index in order {
        let Some(extract) = sources[index].extract.clone() else {
            continue;
        };
        let trimmed = extract.trim();
        if trimmed.is_empty() {
            sources[index].extract = None;
            sources[index].number = None;
            sources[index].unread_reason = Some("the fetch returned no readable text".to_owned());
            continue;
        }
        let remaining = corpus_cap.saturating_sub(used);
        // Admitting a sliver of a page produces a citation with no usable
        // evidence behind it, which is exactly the dishonesty this module
        // exists to prevent. Below one minimum page, stop admitting entirely.
        if remaining < MIN_PAGE_CHARS {
            sources[index].extract = None;
            sources[index].number = None;
            sources[index].unread_reason = Some(format!(
                "read, but dropped from the evidence set: the {corpus_cap}-character research \
                 corpus budget was already full"
            ));
            dropped = dropped.saturating_add(1);
            continue;
        }
        let admitted = bounded_extract(trimmed, remaining.min(page_char_cap()));
        used = used.saturating_add(admitted.chars().count());
        sources[index].extract = Some(admitted);
        sources[index].number = Some(next_number);
        next_number = next_number.saturating_add(1);
    }

    // Anything the loop never numbered must carry a reason; a source with
    // neither a number nor an explanation would appear in the report's
    // unverified list as a bare URL.
    for source in sources.iter_mut() {
        if source.number.is_none() && source.unread_reason.is_none() {
            source.unread_reason = Some(NOT_ATTEMPTED.to_owned());
        }
    }
    dropped
}

/// Coverage facts computed from the finished source set.
#[must_use]
pub fn coverage_of(
    sources: &[ResearchSource],
    planned: usize,
    with_results: usize,
    search_failures: usize,
    gate: GateReport,
    deadline_hit: bool,
    corpus_exhausted: bool,
) -> Coverage {
    let read = sources.iter().filter(|source| source.is_read()).count();
    Coverage {
        planned,
        with_results,
        search_failures,
        found: sources.len(),
        filtered: gate.filtered,
        relevance_fallback: gate.fallback_used,
        read,
        unread: sources.len().saturating_sub(read),
        deadline_hit,
        corpus_exhausted,
    }
}

/// The coverage facts as one paragraph, stated plainly enough that the model
/// cannot paraphrase them into "comprehensive research".
///
/// This is the honesty floor: the report is required to reproduce these numbers,
/// so "2 of 6 sub-queries returned usable sources" reaches the user even when
/// the findings themselves read confidently.
#[must_use]
pub fn coverage_statement(coverage: &Coverage) -> String {
    let mut statement = String::new();
    let _ = write!(
        statement,
        "{} of {} planned sub-queries returned usable results",
        coverage.with_results, coverage.planned
    );
    if coverage.search_failures > 0 {
        let _ = write!(
            statement,
            " ({} search call(s) failed outright)",
            coverage.search_failures
        );
    }
    let _ = write!(
        statement,
        ". {} unique sources were found; {} were read successfully and {} could not be read",
        coverage.found, coverage.read, coverage.unread
    );
    if coverage.filtered > 0 {
        // Named separately from the general unread count so "found 16, read 2"
        // cannot be misread as "14 pages failed to load". Nothing failed: those
        // pages were never about this question, so they were never fetched.
        let _ = write!(
            statement,
            " (of those, {} were set aside as irrelevant to the question before any page was \
             fetched, and none of them is evidence for anything)",
            coverage.filtered
        );
    }
    if coverage.relevance_fallback {
        statement.push_str(
            ". WARNING: no source cleared the relevance bar for this question at all; the \
             best-scoring few were kept as leads rather than returning nothing, so treat every \
             source below as a weak match",
        );
    }
    if coverage.corpus_exhausted {
        statement
            .push_str(". The evidence budget filled before every fetched page could be included");
    }
    if coverage.deadline_hit {
        statement.push_str(
            ". The research time budget ran out before all planned work completed, so this is a \
             PARTIAL evidence base",
        );
    }
    statement.push('.');
    if coverage.read == 0 {
        statement.push_str(
            " NO source was read successfully: there is no evidence base for this question at all.",
        );
    } else if coverage.read < 3 || coverage.with_results * 2 <= coverage.planned {
        statement.push_str(
            " This is a THIN evidence base. Say so in the answer and do not present it as \
             thorough research.",
        );
    }
    statement
}

// ---------------------------------------------------------------------------
// Phase 5 — synthesis prompt + emission (pure)
// ---------------------------------------------------------------------------

/// Build the synthesis prompt: numbered read passages, an explicitly uncitable
/// unread list, the coverage facts, and the report contract.
#[must_use]
pub fn synthesis_prompt(question: &str, sources: &[ResearchSource], coverage: &Coverage) -> String {
    let mut prompt = String::with_capacity(4_096);
    prompt.push_str(
        "You are writing a research report from sources that were fetched for you. Use ONLY \
         these sources.\n\n",
    );
    let _ = writeln!(prompt, "QUESTION: {}\n", question.trim());

    prompt.push_str(
        "SOURCES READ (the text below is each page's own content, truncated where noted). These \
         are the ONLY numbers that exist — you may cite [1], [2], … exactly as listed:\n\n",
    );
    let mut numbered: Vec<&ResearchSource> = sources.iter().filter(|s| s.is_read()).collect();
    numbered.sort_by_key(|source| source.number.unwrap_or(usize::MAX));
    if numbered.is_empty() {
        prompt.push_str("(none — no page could be read)\n\n");
    } else {
        for source in numbered {
            let number = source.number.unwrap_or_default();
            let _ = writeln!(prompt, "[{number}] {} — {}", source.title, source.url);
            let _ = writeln!(
                prompt,
                "{}\n",
                source.extract.as_deref().unwrap_or_default()
            );
        }
    }

    let unread: Vec<&ResearchSource> = sources.iter().filter(|s| !s.is_read()).collect();
    if !unread.is_empty() {
        prompt.push_str(
            "FOUND BUT NOT READ — search returned these, but their text is NOT available to you: \
             either it could not be retrieved, or the source was set aside as irrelevant to the \
             question (the reason is stated per line). They have NO citation number. You may not \
             cite them, quote them, or state anything as fact on their behalf; you may only name \
             them as unverified leads, and a source marked irrelevant is not even a lead:\n",
        );
        for source in unread {
            let _ = writeln!(
                prompt,
                "- {} — {} ({})",
                source.title,
                source.url,
                source.unread_reason.as_deref().unwrap_or("not read")
            );
        }
        prompt.push('\n');
    }

    let _ = writeln!(prompt, "COVERAGE: {}\n", coverage_statement(coverage));

    prompt.push_str(
        "Write the report in the SAME LANGUAGE as the question, in Markdown, with these \
         sections:\n\
         1. A direct answer to the question in 2-4 sentences, opening with how well the \
         evidence actually supports it.\n\
         2. `## Funn` — the substantive findings, under whatever sub-headings the question \
         needs.\n\
         3. `## Kunne ikke verifiseres` — REQUIRED, never omitted. State every part of the \
         question these sources do not answer, restate the coverage numbers above, and name the \
         sources that could not be read. If nothing is missing, say that explicitly.\n\
         4. `## Kilder` — the numbered list of the sources you cited, as `[n] title — url`.\n\n\
         Rules, in priority order:\n\
         - Every factual claim carries an inline `[n]` pointing at a numbered source above. A \
         sentence without one must be visibly your own inference, not a fact.\n\
         - NEVER invent a source, a number, or a URL. Only the numbers listed above exist.\n\
         - NEVER present anything from FOUND BUT NOT READ as verified.\n\
         - If the evidence is thin, say so first and keep the report short. Do not pad it to \
         look complete.\n",
    );
    prompt
}

/// Citation events for every source, read and unread.
///
/// Read sources get `dr-{n}`, which is exactly the `[n]` the report cites — the
/// client can therefore resolve a marker to a Kilder row. Unread sources get
/// `dr-unread-{k}`: they are surfaced (the user should see what search found)
/// but can never be mistaken for a numbered citation.
#[must_use]
pub fn citation_events(sources: &[ResearchSource]) -> Vec<ChatEvent> {
    let mut read: Vec<&ResearchSource> = sources.iter().filter(|s| s.is_read()).collect();
    read.sort_by_key(|source| source.number.unwrap_or(usize::MAX));

    let mut events: Vec<ChatEvent> = read
        .iter()
        .map(|source| ChatEvent::Citation {
            id: format!("dr-{}", source.number.unwrap_or_default()),
            title: source.title.clone(),
            url: source.url.clone(),
            snippet: source.snippet.clone(),
        })
        .collect();

    for (index, source) in sources.iter().filter(|s| !s.is_read()).enumerate() {
        events.push(ChatEvent::Citation {
            id: format!("dr-unread-{}", index + 1),
            title: source.title.clone(),
            url: source.url.clone(),
            // The reason travels in the snippet because `Citation` has no
            // status field: without it a Kilder row for an unreadable page is
            // indistinguishable from one that grounded a claim.
            snippet: unread_citation_snippet(source),
        });
    }
    events
}

/// Snippet for an unread source: the reason first, then whatever the search
/// engine said, so the row reads as a lead rather than as evidence.
fn unread_citation_snippet(source: &ResearchSource) -> String {
    let reason = source.unread_reason.as_deref().unwrap_or("not read");
    if source.snippet.trim().is_empty() {
        format!("[not read: {reason}]")
    } else {
        format!("[not read: {reason}] {}", source.snippet)
    }
}

/// Artifact id for a turn's report.
///
/// Request-scoped, not thread-scoped: two research questions in one thread are
/// two different documents, and giving them a shared id would file the second
/// as "version 2" of the first — a revision history the user never asked for
/// and cannot interpret.
#[must_use]
pub fn report_artifact_id(request_id: &str) -> String {
    format!("research-{request_id}")
}

/// Title shown on the artifact card. Norwegian-first to match the product; the
/// report body follows the question's own language.
#[must_use]
pub fn report_artifact_title(question: &str) -> String {
    const MAX_TITLE_CHARS: usize = 60;
    let collapsed = question.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "Dyp research".to_owned();
    }
    if collapsed.chars().count() <= MAX_TITLE_CHARS {
        return format!("Dyp research: {collapsed}");
    }
    let clipped: String = collapsed.chars().take(MAX_TITLE_CHARS).collect();
    format!("Dyp research: {}\u{2026}", clipped.trim_end())
}

/// The compact receipt the `deep_research` tool result carries.
///
/// Same pattern as [`crate::tool_loop::tool_artifact_events`]: the rich payload
/// (report text, page extracts) travels in the artifact and citation events,
/// while the conversation and the tool timeline see only counts. A tool result
/// carrying the full report would be paid for again on every later round.
#[must_use]
pub fn research_receipt(plan: &[String], coverage: &Coverage, report_chars: usize) -> Value {
    serde_json::json!({
        "sub_queries": plan,
        "sub_queries_with_results": coverage.with_results,
        "search_failures": coverage.search_failures,
        "sources_found": coverage.found,
        "sources_read": coverage.read,
        "sources_unread": coverage.unread,
        // Why `found` and `read` can differ by a lot without anything being
        // broken. Without this the receipt invites the model to explain a gap it
        // has no information about.
        "sources_filtered_irrelevant": coverage.filtered,
        "relevance_fallback_used": coverage.relevance_fallback,
        "report_chars": report_chars,
        "deadline_hit": coverage.deadline_hit,
        "corpus_budget_exhausted": coverage.corpus_exhausted,
        "caps": {
            "max_sub_queries": max_sub_queries(),
            "max_pages": max_pages(),
            "page_chars": page_char_cap(),
            "corpus_chars": corpus_char_cap(),
            "wall_clock_secs": wall_clock_secs(),
        },
    })
}

/// The context message appended to the conversation when a report was produced.
///
/// The report is also emitted as an artifact the user keeps; this hands the same
/// text to the answering model with the citation contract restated, so the
/// streamed answer preserves the `[n]` markers instead of paraphrasing them
/// away.
///
/// Deliberately does NOT tell the model the report is "open in the artifact
/// panel": the `artifacts` event family is opt-in per request, so that would be
/// a claim about the client's UI state the gateway cannot actually guarantee —
/// and a model that repeats it would be pointing the user at a panel that may
/// hold nothing.
#[must_use]
pub fn report_context_message(question: &str, report: &str, coverage: &Coverage) -> String {
    let mut content = String::with_capacity(report.len() + 1_024);
    content.push_str(
        "DEEP RESEARCH COMPLETE. A cited research report was produced for the user's request. \
         Present it as your answer.\n\n",
    );
    let _ = writeln!(content, "Request: {}\n", question.trim());
    let _ = writeln!(content, "COVERAGE: {}\n", coverage_statement(coverage));
    content.push_str("REPORT:\n");
    content.push_str(report.trim());
    content.push_str(
        "\n\nRules for your reply:\n\
         - Reproduce the report's substance, KEEPING every inline `[n]` marker exactly as \
         written. The markers resolve to the sources in the user's Kilder tab; dropping or \
         renumbering them breaks that link.\n\
         - Keep the \"could not be verified\" section and the coverage numbers. Do not upgrade \
         a thin evidence base into a confident one.\n\
         - Add NO claim that is not in the report, and NO citation number the report does not \
         use.\n\
         - Do not run more web searches unless a specific figure the user asked for is missing \
         from the report; if it is missing, say so rather than searching speculatively.\n",
    );
    content
}

/// The context message appended when synthesis could not run or failed.
///
/// Hands the corpus and the report contract straight to the answering model
/// instead of the finished report: the evidence is real and already paid for, so
/// discarding it because one inference failed would be the wrong degradation.
#[must_use]
pub fn unsynthesized_context_message(
    question: &str,
    sources: &[ResearchSource],
    coverage: &Coverage,
    reason: &str,
) -> String {
    let mut content = String::with_capacity(4_096);
    let _ = writeln!(
        content,
        "DEEP RESEARCH PARTIAL: the research phase ran but the report could not be composed \
         ({reason}). The evidence below is real and was fetched for this request — write the \
         report yourself from it, following the same contract.\n"
    );
    content.push_str(&synthesis_prompt(question, sources, coverage));
    content
}

/// The context message appended when the pipeline gathered nothing at all.
///
/// Deliberately not silent: without this the turn would stream a normal
/// ungrounded answer after the user pressed a button labelled "Dyp research",
/// with no indication that the research never happened.
#[must_use]
pub fn no_evidence_context_message(question: &str, coverage: &Coverage, detail: &str) -> String {
    format!(
        "DEEP RESEARCH FAILED: the user requested deep research on \"{}\" and the pipeline could \
         not gather any readable source. {detail}\n{}\n\
         Answer only from what you can actually support, and state plainly and early that the \
         web research did not succeed, so nothing in your answer is web-verified. Do NOT present \
         recalled information as researched, and do NOT invent citations. Offer to retry.",
        question.trim(),
        coverage_statement(coverage),
    )
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Resolves when the turn is cancelled or the research deadline passes.
async fn stop_signal(cancel: &AtomicBool, deadline: Instant) -> StopReason {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return StopReason::Cancelled;
        }
        let now = Instant::now();
        if now >= deadline {
            return StopReason::Deadline;
        }
        tokio::time::sleep(CANCEL_POLL_INTERVAL.min(deadline - now)).await;
    }
}

/// Run `work` unless the turn is cancelled or the deadline passes first.
///
/// `biased` puts the stop check first so a flag that is ALREADY set short-
/// circuits without starting the phase — which is what makes a Stop pressed
/// during the search phase actually skip the read phase instead of merely
/// finishing sooner.
async fn guarded<T, F>(cancel: &AtomicBool, deadline: Instant, work: F) -> Result<T, StopReason>
where
    F: std::future::Future<Output = T>,
{
    tokio::select! {
        biased;
        reason = stop_signal(cancel, deadline) => Err(reason),
        output = work => Ok(output),
    }
}

/// Parse one audited `web_search` outcome into hits.
///
/// `score` is read opportunistically. Quarry DOES return a semantic-reranker
/// relevance per hit and `quarry::project_search_result` now keeps it, but the
/// shared `web_search` tool arm in `tool_loop` projects each result down to
/// `{url, title, snippet}` before this function ever sees it — so today the
/// field is absent and every hit arrives unscored. That is a one-line change in
/// a file this work does not own; reading the field here means the gate starts
/// using Quarry's own judgement the moment it lands, with no further change.
/// Until then the gate runs on lexical overlap and domain class alone, which is
/// exactly what [`crate::relevance`] is built to do without a provider score.
// reason: reranker scores are in [0,1]; f64→f32 loses nothing at that magnitude
#[allow(clippy::cast_possible_truncation)]
fn hits_from_search_output(sub_query: usize, output: &str) -> Vec<SearchHit> {
    let Ok(items) = serde_json::from_str::<Vec<Value>>(output) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(rank, item)| {
            let url = item.get("url")?.as_str()?.trim();
            if url.is_empty() {
                return None;
            }
            Some(SearchHit {
                sub_query,
                rank,
                url: url.to_owned(),
                title: item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                snippet: item
                    .get("snippet")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                score: item
                    .get("score")
                    .and_then(Value::as_f64)
                    .map(|value| value as f32)
                    .filter(|value| value.is_finite()),
            })
        })
        .collect()
}

/// Extract page text from one audited `fetch_url` outcome.
fn extract_from_fetch_output(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output).ok()?;
    let content = value.get("content")?.as_str()?.trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_owned())
    }
}

/// The whole pipeline: plan → search → read → synthesize → emit.
///
/// # Errors
///
/// Returns `Err` only when a tool action could not be durably audited — the same
/// single hard failure the inline tool loop has. Every other failure (no Quarry
/// edge, dead network, empty results, failed inference, exhausted budget)
/// degrades into a stated fact in the returned context message.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // cohesive pipeline entry — all are request context
pub async fn run_deep_research(
    state: &AppState,
    request_id: &str,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    inference_bearer: &str,
    session_bearer: &str,
    zdr: bool,
    // Turn's minimum privacy tier (wire numeric). Every derived research
    // inference inherits it: a planning or synthesis call must never reach a
    // provider the main chat chain would refuse.
    min_privacy_tier: i32,
    model: &str,
    base_messages: Vec<ChatMessage>,
    question: &str,
    cancel: &AtomicBool,
    sink: Option<&crate::sse_events::RichEventSink>,
) -> Result<DeepResearchOutcome, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(wall_clock_secs());
    let mut events = match sink {
        Some(sink) => ResearchEvents::Live(sink),
        None => ResearchEvents::Buffered(Vec::new()),
    };
    let mut tool_successes: u32 = 0;
    let mut tool_failures: u32 = 0;

    // One synthetic tool call frames the whole pipeline in the tool timeline.
    let pipeline_call_id = format!("{request_id}-deep-research");
    events
        .push(ChatEvent::ToolCall {
            id: pipeline_call_id.clone(),
            name: DEEP_RESEARCH_TOOL.to_owned(),
            args: serde_json::json!({ "question": question }),
        })
        .await;

    // Gate: no Quarry edge means no web at all. Say so instead of running four
    // phases that can only fail.
    if !state.quarry.available() {
        let coverage = Coverage::default();
        return Ok(finish_without_evidence(
            events,
            base_messages,
            question,
            &coverage,
            &pipeline_call_id,
            &[],
            "The web fetch service (Quarry edge) is not configured for this deployment, so no \
             search or page read was attempted.",
            0,
            1,
        )
        .await);
    }

    // ---- Phase 1: plan ---------------------------------------------------
    events
        .step(
            STEP_PLAN,
            "Planlegger research",
            "Deler spørsmålet i delspørsmål.",
            "active",
        )
        .await;
    let plan_limit = max_sub_queries();
    let planned = guarded(
        cancel,
        deadline,
        plan_sub_queries(
            state,
            request_id,
            org_id,
            inference_bearer,
            zdr,
            min_privacy_tier,
            question,
            plan_limit,
            model,
        ),
    )
    .await;
    let (plan, plan_degraded) = match planned {
        Ok(result) => result,
        Err(StopReason::Cancelled) => {
            return Ok(cancelled_outcome(events, base_messages));
        }
        // The deadline cannot realistically expire during planning (its own
        // timeout is far shorter), but if it does, the question itself is a
        // valid one-item plan. Routed through `parse_sub_queries` rather than
        // `normalize_sub_query` so the plausibility filter still applies: a
        // question that normalizes to nothing would otherwise become an empty
        // sub-query, and `web_search` rejects an empty query — turning a
        // degradation into a failed tool call for no reason.
        Err(StopReason::Deadline) => (parse_sub_queries("", question, plan_limit), true),
    };
    events
        .step(
            STEP_PLAN,
            "Planlegger research",
            &if plan_degraded {
                format!(
                    "Planleggingen feilet – søker på spørsmålet direkte: {}",
                    plan.join(" · ")
                )
            } else {
                format!("{} delspørsmål: {}", plan.len(), plan.join(" · "))
            },
            "done",
        )
        .await;

    // ---- Phase 2: search -------------------------------------------------
    events
        .step(
            STEP_SEARCH,
            "Søker kilder",
            &format!("Kjører {} søk parallelt.", plan.len()),
            "active",
        )
        .await;
    let searched = guarded(
        cancel,
        deadline,
        search_sub_queries(
            state,
            request_id,
            run_id,
            org_id,
            user_id,
            thread_id,
            session_bearer,
            zdr,
            &plan,
        ),
    )
    .await;
    let mut deadline_hit = false;
    let (hits, search_ok, search_failures) = match searched {
        Ok(result) => result?,
        Err(StopReason::Cancelled) => {
            return Ok(cancelled_outcome(events, base_messages));
        }
        Err(StopReason::Deadline) => {
            deadline_hit = true;
            (Vec::new(), 0, plan.len())
        }
    };
    tool_successes = tool_successes.saturating_add(u32::try_from(search_ok).unwrap_or(u32::MAX));
    tool_failures =
        tool_failures.saturating_add(u32::try_from(search_failures).unwrap_or(u32::MAX));
    let mut sources = dedupe_hits(&hits);
    // The relevance gate, before reading and therefore before citing. Filtered
    // sources stay in `sources` (counted, shown, uncitable) but leave
    // `read_order`, so the page budget is never spent on a hit that cannot
    // answer the question.
    let gate = apply_relevance_gate(question, &mut sources);
    let relevant = sources.len().saturating_sub(gate.filtered);
    events
        .step(
            STEP_SEARCH,
            "Søker kilder",
            &format!(
                "{relevant} relevante av {} unike kilder fra {} av {} delspørsmål.",
                sources.len(),
                search_ok,
                plan.len()
            ),
            if relevant == 0 { "error" } else { "done" },
        )
        .await;

    // ---- Phase 3: read ---------------------------------------------------
    let priority = read_order(&sources);
    let to_read = pages_to_read(&priority, max_pages());
    if !to_read.is_empty() && !deadline_hit {
        events
            .step(
                STEP_READ,
                "Leser kilder",
                &format!("Henter {} sider parallelt.", to_read.len()),
                "active",
            )
            .await;
        let read = guarded(
            cancel,
            deadline,
            read_pages(
                state,
                request_id,
                run_id,
                org_id,
                user_id,
                thread_id,
                session_bearer,
                zdr,
                &sources,
                &to_read,
            ),
        )
        .await;
        match read {
            Ok(result) => {
                let (extracts, ok, failed) = result?;
                tool_successes =
                    tool_successes.saturating_add(u32::try_from(ok).unwrap_or(u32::MAX));
                tool_failures =
                    tool_failures.saturating_add(u32::try_from(failed).unwrap_or(u32::MAX));
                for (index, outcome) in extracts {
                    match outcome {
                        Ok(text) => {
                            sources[index].extract = Some(text);
                            sources[index].unread_reason = None;
                        }
                        Err(reason) => {
                            sources[index].extract = None;
                            sources[index].unread_reason = Some(reason);
                        }
                    }
                }
            }
            Err(StopReason::Cancelled) => {
                return Ok(cancelled_outcome(events, base_messages));
            }
            Err(StopReason::Deadline) => {
                deadline_hit = true;
                for &index in &to_read {
                    if sources[index].extract.is_none() {
                        sources[index].unread_reason = Some(
                            "the research time budget ran out before this page was read".to_owned(),
                        );
                    }
                }
            }
        }
    }
    let dropped_for_budget = number_and_bound(&mut sources, &priority, corpus_char_cap());
    let coverage = coverage_of(
        &sources,
        plan.len(),
        search_ok,
        search_failures,
        gate,
        deadline_hit,
        dropped_for_budget > 0,
    );
    events
        .step(
            STEP_READ,
            "Leser kilder",
            &format!(
                "Leste {} av {} valgte sider ({} uten lesbar tekst).",
                coverage.read,
                to_read.len(),
                coverage.unread
            ),
            if coverage.read == 0 { "error" } else { "done" },
        )
        .await;

    // ---- Phase 4: citations ---------------------------------------------
    // Emitted before synthesis so the Kilder tab fills while the report is
    // still being written, rather than everything landing at once at the end.
    let citations = citation_events(&sources);
    let web_citations = u32::try_from(citations.len()).unwrap_or(u32::MAX);
    for citation in citations {
        events.push(citation).await;
    }

    if coverage.read == 0 {
        let detail = if coverage.found == 0 {
            "No search result was returned at all — the web is unreachable from this deployment, \
             or every search call failed."
                .to_owned()
        } else if coverage.filtered == coverage.found {
            // Every hit was off-topic. Saying "none yielded readable text" here
            // would blame the fetch layer for a search-quality problem and send
            // the user looking for a bug that is not there.
            format!(
                "{} source(s) were found, but not one of them was about this question, so none \
                 was read. The search engine returned results for the words, not for the \
                 question.",
                coverage.found
            )
        } else {
            format!(
                "{} source(s) were found ({} set aside as irrelevant) but none of the rest \
                 yielded readable text (typically JavaScript-rendered pages, whose content is not \
                 in the HTML).",
                coverage.found, coverage.filtered
            )
        };
        let mut outcome = finish_without_evidence(
            events,
            base_messages,
            question,
            &coverage,
            &pipeline_call_id,
            &plan,
            &detail,
            tool_successes,
            tool_failures,
        )
        .await;
        outcome.web_citations = web_citations;
        return Ok(outcome);
    }

    // ---- Phase 5: synthesize --------------------------------------------
    events
        .step(
            STEP_SYNTHESIZE,
            "Skriver rapport",
            &format!(
                "Syntetiserer {} leste kilder med kildehenvisninger.",
                coverage.read
            ),
            "active",
        )
        .await;
    let prompt = synthesis_prompt(question, &sources, &coverage);
    let synthesized = guarded(
        cancel,
        deadline,
        synthesize_report(
            state,
            request_id,
            org_id,
            inference_bearer,
            zdr,
            min_privacy_tier,
            model,
            &prompt,
        ),
    )
    .await;
    let (report, resolved_model, synthesis_failure) = match synthesized {
        Ok(Some((report, resolved))) => (Some(report), resolved, None),
        Ok(None) => (None, None, Some("the synthesis inference failed")),
        Err(StopReason::Cancelled) => {
            return Ok(cancelled_outcome(events, base_messages));
        }
        Err(StopReason::Deadline) => (
            None,
            None,
            Some("the research time budget ran out before the report was composed"),
        ),
    };

    let mut messages = base_messages;
    let report_chars = report.as_ref().map_or(0, |text| text.chars().count());
    match (&report, synthesis_failure) {
        (Some(text), _) => {
            let artifact_id = report_artifact_id(request_id);
            let title = report_artifact_title(question);
            let version = state
                .artifact_versions
                .next_version(thread_id, &artifact_id);
            events
                .push(crate::artifacts::artifact_event(
                    &artifact_id,
                    ArtifactKind::Document,
                    &title,
                    text,
                    version,
                ))
                .await;
            events
                .step(
                    STEP_SYNTHESIZE,
                    "Skriver rapport",
                    &format!(
                        "Rapport klar: {report_chars} tegn, {} kilder sitert.",
                        coverage.read
                    ),
                    "done",
                )
                .await;
            messages.push(ChatMessage {
                role: "user".to_owned(),
                content: report_context_message(question, text, &coverage),
                name: String::new(),
            });
        }
        (None, reason) => {
            let reason = reason.unwrap_or("the report could not be composed");
            events
                .step(
                    STEP_SYNTHESIZE,
                    "Skriver rapport",
                    &format!("Rapporten kunne ikke skrives ({reason}) – svarer fra kildene."),
                    "error",
                )
                .await;
            messages.push(ChatMessage {
                role: "user".to_owned(),
                content: unsynthesized_context_message(question, &sources, &coverage, reason),
                name: String::new(),
            });
        }
    }

    let receipt = research_receipt(&plan, &coverage, report_chars);
    events
        .push(ChatEvent::ToolResult {
            id: pipeline_call_id,
            // A run that gathered evidence but could not compose the report is
            // NOT "ok" in the tool timeline. Reporting it as ok would put a green
            // check on the one step that failed, which is the same class of lie
            // as citing a page nobody read.
            status: if report.is_some() {
                "ok".to_owned()
            } else {
                "error".to_owned()
            },
            output: receipt.to_string(),
            error: synthesis_failure.map(str::to_owned),
        })
        .await;

    tracing::debug!(
        %request_id,
        planned = coverage.planned,
        with_results = coverage.with_results,
        found = coverage.found,
        read = coverage.read,
        unread = coverage.unread,
        deadline_hit = coverage.deadline_hit,
        report_chars,
        "deep research complete"
    );

    Ok(DeepResearchOutcome {
        messages,
        events: events.into_buffer(),
        resolved_model,
        any_tool_succeeded: tool_successes > 0,
        tool_successes,
        tool_failures,
        web_citations,
        cancelled: false,
        report,
        coverage,
    })
}

/// Build the outcome for a cancelled run: the conversation untouched, so the
/// caller emits a terminal `stopped` rather than answering from half-gathered
/// evidence the user has already declined to wait for.
fn cancelled_outcome(
    events: ResearchEvents<'_>,
    base_messages: Vec<ChatMessage>,
) -> DeepResearchOutcome {
    DeepResearchOutcome {
        messages: base_messages,
        events: events.into_buffer(),
        resolved_model: None,
        any_tool_succeeded: false,
        tool_successes: 0,
        tool_failures: 0,
        web_citations: 0,
        cancelled: true,
        report: None,
        coverage: Coverage::default(),
    }
}

/// Build the outcome for a run that gathered no readable evidence.
#[allow(clippy::too_many_arguments)] // one call shape, used from two failure points
async fn finish_without_evidence(
    mut events: ResearchEvents<'_>,
    mut messages: Vec<ChatMessage>,
    question: &str,
    coverage: &Coverage,
    pipeline_call_id: &str,
    plan: &[String],
    detail: &str,
    tool_successes: u32,
    tool_failures: u32,
) -> DeepResearchOutcome {
    events
        .step(
            STEP_SYNTHESIZE,
            "Skriver rapport",
            "Ingen lesbare kilder – ingen rapport skrevet.",
            "error",
        )
        .await;
    events
        .push(ChatEvent::ToolResult {
            id: pipeline_call_id.to_owned(),
            status: "error".to_owned(),
            output: research_receipt(plan, coverage, 0).to_string(),
            error: Some(detail.to_owned()),
        })
        .await;
    messages.push(ChatMessage {
        role: "user".to_owned(),
        content: no_evidence_context_message(question, coverage, detail),
        name: String::new(),
    });
    DeepResearchOutcome {
        messages,
        events: events.into_buffer(),
        resolved_model: None,
        any_tool_succeeded: false,
        tool_successes,
        tool_failures,
        web_citations: 0,
        cancelled: false,
        report: None,
        coverage: *coverage,
    }
}

/// One cheap, bounded, non-streaming inference that decomposes the question.
///
/// Returns `(plan, degraded)`. Best-effort in the same posture as
/// `sse::generate_thread_title`: every failure path logs at debug and degrades
/// to searching the question as asked, because a research turn must produce an
/// answer rather than an error.
#[allow(clippy::too_many_arguments)] // request context, mirrors the other phases
async fn plan_sub_queries(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    inference_bearer: &str,
    zdr: bool,
    min_privacy_tier: i32,
    question: &str,
    limit: usize,
    turn_model: &str,
) -> (Vec<String>, bool) {
    let configured =
        std::env::var("DEEP_RESEARCH_PLAN_MODEL").unwrap_or_else(|_| PLAN_MODEL.to_owned());

    let mut raw = plan_once(
        state,
        request_id,
        org_id,
        inference_bearer,
        zdr,
        min_privacy_tier,
        question,
        limit,
        &configured,
    )
    .await;

    // The configured plan tier can be unroutable in a deployment while
    // the turn's own model demonstrably answers (it streamed the reply).
    // Degrading the whole feature to "search the raw question" because a
    // *cheaper* tier is unavailable trades the entire decomposition for
    // nothing, so retry once on the model this turn is already using.
    if should_retry_plan_on_turn_model(&raw, &configured, turn_model) {
        tracing::warn!(
            %request_id,
            plan_model = %configured,
            %turn_model,
            "deep research plan model produced no content; retrying on the turn's model"
        );
        raw = plan_once(
            state,
            request_id,
            org_id,
            inference_bearer,
            zdr,
            min_privacy_tier,
            question,
            limit,
            turn_model,
        )
        .await;
    }

    let degraded = raw.trim().is_empty();
    if degraded {
        tracing::warn!(
            %request_id,
            plan_model = %configured,
            %turn_model,
            "deep research planning degraded; searching the question as asked"
        );
    }
    (parse_sub_queries(&raw, question, limit), degraded)
}

/// Whether an empty plan warrants one retry on the turn's own model.
///
/// True only when the configured plan model produced nothing AND the
/// turn's model is a different, usable id — retrying the same id would
/// just spend the wall clock twice for the same empty answer.
fn should_retry_plan_on_turn_model(raw: &str, configured: &str, turn_model: &str) -> bool {
    raw.trim().is_empty() && !turn_model.trim().is_empty() && turn_model != configured
}

/// One bounded planning inference. Returns the raw content, or an empty
/// string for every failure mode — each of which logs its own cause.
#[allow(clippy::too_many_arguments)] // request context, mirrors plan_sub_queries
async fn plan_once(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    inference_bearer: &str,
    zdr: bool,
    min_privacy_tier: i32,
    question: &str,
    limit: usize,
    model: &str,
) -> String {
    let mut client = state.inference_client.clone();
    let response = tokio::time::timeout(
        PLAN_TIMEOUT,
        client.infer(authorized(
            InferRequest {
                request_id: format!("{request_id}-research-plan"),
                org_id: org_id.to_owned(),
                model: model.to_owned(),
                messages: vec![ChatMessage {
                    role: "user".to_owned(),
                    content: plan_prompt(question, limit),
                    name: String::new(),
                }],
                // Low temperature: a plan should be a stable decomposition, not
                // a creative one.
                temperature: 0.2,
                max_tokens: PLAN_MAX_TOKENS,
                zdr,
                // Derived research calls inherit the turn's floor: planning
                // must never reach a provider the main chain would refuse.
                min_privacy_tier,
                ..Default::default()
            },
            inference_bearer,
        )),
    )
    .await;
    match response {
        Ok(Ok(resp)) => {
            let content = resp.into_inner().content;
            // A successful-but-empty response used to be the ONE path
            // here that logged nothing, which is why an always-degrading
            // planner survived a debug-level live run undetected.
            if content.trim().is_empty() {
                tracing::warn!(
                    %request_id,
                    %model,
                    "deep research plan inference succeeded but returned empty content"
                );
            }
            content
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, %request_id, %model, "deep research plan inference failed");
            String::new()
        }
        Err(_elapsed) => {
            tracing::debug!(%request_id, %model, "deep research plan inference timed out");
            String::new()
        }
    }
}

/// Run every sub-query CONCURRENTLY through the audited `web_search` path.
///
/// Returns `(hits, sub_queries_with_results, failures)`. Concurrency is what
/// makes the phase affordable in wall clock: six sequential searches cost six
/// round trips for no correctness gain, exactly as in
/// `tool_loop::run_tool_rounds`.
#[allow(clippy::too_many_arguments)] // request context, mirrors dispatch_web_tool_audited
async fn search_sub_queries(
    state: &AppState,
    request_id: &str,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    session_bearer: &str,
    zdr: bool,
    plan: &[String],
) -> Result<(Vec<SearchHit>, usize, usize), &'static str> {
    let dispatched = futures::future::join_all(plan.iter().enumerate().map(|(index, query)| {
        let call = ToolCall {
            id: format!("{request_id}-dr-search-{index}"),
            name: "web_search".to_owned(),
            arguments_json: serde_json::json!({
                "query": query,
                "limit": SEARCH_RESULTS_PER_SUB_QUERY,
                "intent": "research",
            })
            .to_string(),
        };
        async move {
            crate::tool_loop::dispatch_web_tool_audited(
                state,
                request_id,
                run_id,
                org_id,
                user_id,
                thread_id,
                session_bearer,
                zdr,
                &call,
            )
            .await
        }
    }))
    .await;

    let mut hits: Vec<SearchHit> = Vec::new();
    let mut with_results = 0usize;
    let mut failures = 0usize;
    for (index, result) in dispatched.into_iter().enumerate() {
        let outcome = result?;
        if let Some(error) = &outcome.error {
            tracing::debug!(%request_id, sub_query = index, %error, "deep research sub-query search failed");
            failures = failures.saturating_add(1);
            continue;
        }
        let parsed = hits_from_search_output(index, &outcome.output);
        if parsed.is_empty() {
            // A search that returned zero rows is not a failure of the call,
            // but it is not coverage either — counting it as coverage is how a
            // report ends up claiming six sub-queries were answered.
            continue;
        }
        with_results = with_results.saturating_add(1);
        hits.extend(parsed);
    }
    Ok((hits, with_results, failures))
}

/// Fetch the selected pages CONCURRENTLY through the audited `fetch_url` path.
///
/// Returns `(per-page results, ok, failed)`. A page that returns no readable
/// text is an `Err` carrying the reason, not an empty success: reporting a page
/// we could not read as read is the single failure this whole module exists to
/// prevent.
#[allow(clippy::too_many_arguments)] // request context, mirrors search_sub_queries
async fn read_pages(
    state: &AppState,
    request_id: &str,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    thread_id: &str,
    session_bearer: &str,
    zdr: bool,
    sources: &[ResearchSource],
    selected: &[usize],
) -> Result<(Vec<(usize, Result<String, String>)>, usize, usize), &'static str> {
    let dispatched =
        futures::future::join_all(selected.iter().enumerate().map(|(slot, &source_index)| {
            let call = ToolCall {
                id: format!("{request_id}-dr-read-{slot}"),
                name: "fetch_url".to_owned(),
                arguments_json: serde_json::json!({ "url": sources[source_index].url }).to_string(),
            };
            async move {
                let outcome = crate::tool_loop::dispatch_web_tool_audited(
                    state,
                    request_id,
                    run_id,
                    org_id,
                    user_id,
                    thread_id,
                    session_bearer,
                    zdr,
                    &call,
                )
                .await;
                (source_index, outcome)
            }
        }))
        .await;

    let mut results: Vec<(usize, Result<String, String>)> = Vec::new();
    let mut ok = 0usize;
    let mut failed = 0usize;
    for (source_index, result) in dispatched {
        let outcome = result?;
        if outcome.error.is_some() {
            failed = failed.saturating_add(1);
            // `fetch_url` already turns an empty extraction into an error whose
            // message names the cause (JS-rendered page). Keep the upstream
            // wording — it is what the report shows the user.
            results.push((
                source_index,
                Err(outcome
                    .error
                    .unwrap_or_else(|| "the page could not be fetched".to_owned())),
            ));
            continue;
        }
        if let Some(text) = extract_from_fetch_output(&outcome.output) {
            ok = ok.saturating_add(1);
            results.push((source_index, Ok(bounded_extract(&text, page_char_cap()))));
        } else {
            failed = failed.saturating_add(1);
            results.push((
                source_index,
                Err(
                    "the fetch succeeded but extracted no readable text (the page is most \
                     likely rendered by JavaScript)"
                        .to_owned(),
                ),
            ));
        }
    }
    Ok((results, ok, failed))
}

/// One bounded inference that writes the report. Returns the report plus the
/// concrete model that produced it, or `None` on any failure.
async fn synthesize_report(
    state: &AppState,
    request_id: &str,
    org_id: &str,
    inference_bearer: &str,
    zdr: bool,
    min_privacy_tier: i32,
    model: &str,
    prompt: &str,
) -> Option<(String, Option<String>)> {
    let mut client = state.inference_client.clone();
    let response = tokio::time::timeout(
        SYNTHESIS_TIMEOUT,
        client.infer(authorized(
            InferRequest {
                request_id: format!("{request_id}-research-report"),
                org_id: org_id.to_owned(),
                model: model.to_owned(),
                messages: vec![ChatMessage {
                    role: "user".to_owned(),
                    content: prompt.to_owned(),
                    name: String::new(),
                }],
                // Low temperature: the report must follow the sources, and a
                // creative sampler is exactly how a citation drifts off the
                // passage it came from.
                temperature: 0.2,
                max_tokens: report_token_budget(),
                zdr,
                // Report synthesis inherits the turn's floor for the same
                // reason planning does.
                min_privacy_tier,
                ..Default::default()
            },
            inference_bearer,
        )),
    )
    .await;
    match response {
        Ok(Ok(resp)) => {
            let resp = resp.into_inner();
            let content = resp.content.trim().to_owned();
            if content.is_empty() {
                tracing::debug!(%request_id, "deep research synthesis returned empty content");
                return None;
            }
            let resolved =
                Some(resp.model_used).filter(|model_used: &String| !model_used.trim().is_empty());
            Some((content, resolved))
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, %request_id, "deep research synthesis inference failed");
            None
        }
        Err(_elapsed) => {
            tracing::debug!(%request_id, "deep research synthesis inference timed out");
            None
        }
    }
}

/// Attach the delegated inference bearer. inference-core rejects a bare
/// `Infer`, which is what silently killed every model-decided tool round in
/// production before `tool_loop` started forwarding it.
fn authorized<T>(value: T, bearer: &str) -> tonic::Request<T> {
    let mut request = tonic::Request::new(value);
    if bearer.is_empty() {
        return request;
    }
    if let Ok(header) = format!("Bearer {bearer}").parse() {
        request.metadata_mut().insert("authorization", header);
    }
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(url: &str, sub_queries: Vec<usize>, rank: usize) -> ResearchSource {
        ResearchSource {
            number: None,
            url: url.to_owned(),
            title: format!("Title of {url}"),
            snippet: "snippet".to_owned(),
            sub_queries,
            best_rank: rank,
            provider_score: None,
            // Ungated: the same neutral value `dedupe_hits` produces, so tests
            // about ordering and numbering are not accidentally testing the gate.
            relevance: 1.0,
            filtered: false,
            extract: None,
            unread_reason: Some(NOT_ATTEMPTED.to_owned()),
        }
    }

    /// A source the relevance gate set aside.
    fn filtered_source(url: &str, relevance: f32) -> ResearchSource {
        let mut source = source(url, vec![0], 0);
        source.relevance = relevance;
        source.filtered = true;
        source.unread_reason = Some("filtered as irrelevant to the question (test)".to_owned());
        source
    }

    fn read_source(url: &str, text: &str) -> ResearchSource {
        let mut source = source(url, vec![0], 0);
        source.extract = Some(text.to_owned());
        source.unread_reason = None;
        source
    }

    // --- plan parsing ----------------------------------------------------

    #[test]
    fn plan_parsing_reads_a_bare_json_array() {
        // The prompt asks for exactly this shape, so it must be the happy path.
        let plan = parse_sub_queries(
            r#"["oslo befolkning 2026", "oslo befolkningsvekst prognose"]"#,
            "hvor mange bor i oslo",
            6,
        );
        assert_eq!(
            plan,
            vec!["oslo befolkning 2026", "oslo befolkningsvekst prognose"]
        );
    }

    #[test]
    fn plan_parsing_reads_a_fenced_object_and_an_array_of_objects() {
        // Models wrap JSON in a fence and nest it under a key unprompted; both
        // are good-faith answers and must not cost the turn its plan.
        let fenced = parse_sub_queries(
            "```json\n{\"queries\": [\"a query\", \"b query\"]}\n```",
            "q",
            6,
        );
        assert_eq!(fenced, vec!["a query", "b query"]);

        let objects = parse_sub_queries(r#"[{"query":"c query"},{"query":"d query"}]"#, "q", 6);
        assert_eq!(objects, vec!["c query", "d query"]);
    }

    #[test]
    fn plan_parsing_takes_marked_list_items_and_drops_the_prose_around_them() {
        // The most common non-JSON answer: a numbered list with a lead-in and a
        // sign-off. Keeping the prose would send "Here are the queries:" to a
        // search engine and burn one of a handful of sub-query slots on it.
        let plan = parse_sub_queries(
            "Here are the sub-queries:\n1. bring fraktpriser 2026\n2. posten fraktpriser 2026\n\
             Let me know if you want more.",
            "hva koster frakt",
            6,
        );
        assert_eq!(
            plan,
            vec!["bring fraktpriser 2026", "posten fraktpriser 2026"]
        );
    }

    #[test]
    fn a_malformed_plan_degrades_to_the_users_own_question() {
        // The single most important degradation in the module: an empty plan
        // searches nothing and finds nothing, and a pipeline that then wrote a
        // report would be writing it over zero evidence. Searching the question
        // as asked is still real research.
        for malformed in ["", "   ", "I'm sorry, I can't help with that.:", "{}", "[]"] {
            let plan = parse_sub_queries(malformed, "hvem eier Aquatiq", 6);
            assert!(
                !plan.is_empty(),
                "plan must never be empty (input {malformed:?})"
            );
            assert!(
                plan.iter().any(|query| query.contains("Aquatiq")),
                "fallback must be the user's own question (input {malformed:?}, got {plan:?})"
            );
        }
    }

    #[test]
    fn plan_parsing_dedupes_case_insensitively_and_enforces_the_cap() {
        // Duplicate sub-queries cost a search round trip and a share of the
        // corpus budget to re-find the same pages; the cap is a spend limit, so
        // a model returning twenty must not get twenty searches.
        let plan = parse_sub_queries(
            r#"["Same Query", "same query", "  SAME QUERY  ", "b", "c", "d", "e", "f", "g"]"#,
            "q",
            3,
        );
        assert_eq!(plan, vec!["Same Query", "b", "c"]);
    }

    #[test]
    fn a_sub_query_is_capped_on_a_char_boundary() {
        // Multi-byte characters are the norm in Norwegian; a byte-wise cut here
        // would panic on a slice boundary instead of shortening a query.
        let long = "æøå ".repeat(200);
        let plan = parse_sub_queries(&format!("[\"{long}\"]"), "q", 6);
        assert_eq!(plan.len(), 1);
        assert!(plan[0].chars().count() <= MAX_SUB_QUERY_CHARS);
    }

    // --- dedupe ----------------------------------------------------------

    #[test]
    fn url_dedupe_collapses_scheme_www_fragment_and_trailing_slash() {
        // All four of these are one page. Counting them as four would inflate
        // the citation list AND the corroboration signal with one document.
        let key = normalize_url_key("https://www.ssb.no/statbank/");
        assert_eq!(key, "ssb.no/statbank");
        assert_eq!(normalize_url_key("http://ssb.no/statbank"), key);
        assert_eq!(normalize_url_key("https://SSB.no/statbank#tabell"), key);
        assert_eq!(normalize_url_key("  https://www.ssb.no/statbank  "), key);
        // A different path is a different source.
        assert_ne!(normalize_url_key("https://ssb.no/statbank/other"), key);
    }

    #[test]
    fn dedupe_records_every_sub_query_that_surfaced_a_url() {
        // Cross-sub-query corroboration is the read-priority signal, so it must
        // survive dedupe rather than being overwritten by the last hit.
        let hits = vec![
            SearchHit {
                sub_query: 0,
                rank: 3,
                url: "https://a.no/x".into(),
                title: String::new(),
                snippet: String::new(),
                score: None,
            },
            SearchHit {
                sub_query: 1,
                rank: 1,
                url: "https://www.a.no/x/".into(),
                title: "A page".into(),
                snippet: "about x".into(),
                score: Some(0.8),
            },
            SearchHit {
                sub_query: 1,
                rank: 0,
                url: "https://b.no/y".into(),
                title: "B".into(),
                snippet: String::new(),
                score: None,
            },
        ];
        let sources = dedupe_hits(&hits);
        assert_eq!(sources.len(), 2, "the two a.no URLs are one source");
        assert_eq!(sources[0].sub_queries, vec![0, 1]);
        // The reranker only scores the head of each response, so the same URL is
        // routinely scored under one sub-query and unscored under another. One
        // sub-query's judgement must not be lost to another's silence.
        assert_eq!(sources[0].provider_score, Some(0.8));
        assert_eq!(sources[1].provider_score, None);
        assert_eq!(sources[0].best_rank, 1, "keeps the best rank seen");
        // Metadata is backfilled from the richer duplicate; a URL is a poor
        // title when a real one exists.
        assert_eq!(sources[0].title, "A page");
        assert_eq!(sources[0].snippet, "about x");
        assert_eq!(sources[1].sub_queries, vec![1]);
        // A hit with no title falls back to its URL so the Kilder row is never
        // blank.
        assert_eq!(sources[1].title, "B");
    }

    #[test]
    fn read_order_prefers_corroboration_then_rank_then_discovery_order() {
        let sources = vec![
            source("https://a.no", vec![0], 0),
            source("https://b.no", vec![0, 1, 2], 4),
            source("https://c.no", vec![0, 1], 2),
            source("https://d.no", vec![0, 1], 1),
        ];
        // b (3 sub-queries) → d and c (2 each, d ranked better) → a (1).
        assert_eq!(read_order(&sources), vec![1, 3, 2, 0]);
    }

    // --- numbering / caps -------------------------------------------------

    #[test]
    fn numbering_covers_only_sources_that_were_actually_read() {
        // The module's core invariant. A number is a licence to cite; handing
        // one to a page we never read is how a report cites a source it has
        // never seen a word of.
        let mut sources = vec![
            read_source("https://read.no", "real page text"),
            source("https://unread.no", vec![0], 1),
        ];
        sources[1].unread_reason = Some("JavaScript-rendered".to_owned());
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);

        assert_eq!(sources[0].number, Some(1));
        assert!(sources[0].is_read());
        assert_eq!(sources[1].number, None, "an unread source gets no number");
        assert!(!sources[1].is_read());
        assert_eq!(
            sources[1].unread_reason.as_deref(),
            Some("JavaScript-rendered"),
            "the stated reason must survive numbering — the report shows it"
        );
    }

    #[test]
    fn an_empty_extract_is_treated_as_unread_not_as_a_blank_source() {
        // Quarry returns a clean 200 with no text for JS-rendered pages. A
        // whitespace-only extract must not become citable source [1].
        let mut sources = vec![read_source("https://blank.no", "   \n  ")];
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);
        assert_eq!(sources[0].number, None);
        assert!(sources[0].extract.is_none());
        assert!(sources[0]
            .unread_reason
            .as_deref()
            .unwrap_or_default()
            .contains("no readable text"));
    }

    #[test]
    fn citation_event_ids_match_the_report_numbering() {
        // The report cites `[n]`; the client resolves it against the Kilder
        // rows. If the id scheme and the numbering ever diverge, every citation
        // in every report silently points at the wrong source.
        let mut sources = vec![
            read_source("https://one.no", "text one"),
            read_source("https://two.no", "text two"),
        ];
        sources[1].sub_queries = vec![0, 1]; // corroborated → read first → [1]
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);

        let events = citation_events(&sources);
        assert_eq!(events.len(), 2);
        for event in &events {
            let ChatEvent::Citation { id, url, .. } = event else {
                panic!("expected a citation, got {event:?}");
            };
            let number: usize = id
                .strip_prefix("dr-")
                .and_then(|rest| rest.parse().ok())
                .unwrap_or_else(|| panic!("citation id {id} is not dr-<n>"));
            let matched = sources
                .iter()
                .find(|source| source.number == Some(number))
                .expect("every emitted number belongs to a source");
            assert_eq!(&matched.url, url);
        }
        // Emitted in citation order, so [1] is the first Kilder row.
        assert!(matches!(&events[0], ChatEvent::Citation { id, .. } if id == "dr-1"));
        assert!(matches!(&events[1], ChatEvent::Citation { id, .. } if id == "dr-2"));
    }

    #[test]
    fn unread_sources_are_surfaced_but_never_look_like_a_numbered_citation() {
        // The user should still see what search found — but a Kilder row for a
        // page we could not open must not be indistinguishable from one that
        // grounded a claim.
        let mut sources = vec![
            read_source("https://read.no", "text"),
            source("https://unread.no", vec![0], 1),
        ];
        sources[1].unread_reason = Some("JavaScript-rendered".to_owned());
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);

        let events = citation_events(&sources);
        assert_eq!(events.len(), 2);
        let ChatEvent::Citation { id, snippet, .. } = &events[1] else {
            panic!("expected a citation");
        };
        assert_eq!(id, "dr-unread-1");
        assert!(
            id.strip_prefix("dr-")
                .and_then(|rest| rest.parse::<usize>().ok())
                .is_none(),
            "an unread id must not parse as a citation number"
        );
        assert!(snippet.starts_with("[not read: JavaScript-rendered]"));
    }

    #[test]
    fn the_page_budget_truncates_the_fetch_list_in_priority_order() {
        // The fetch phase spends real network time and real audit rows per page,
        // so "we only fetch N" must be enforced somewhere a test can hold it.
        let order = vec![4, 1, 0, 3, 2];
        assert_eq!(pages_to_read(&order, 3), vec![4, 1, 0]);
        // A budget above the candidate count fetches everything, not more.
        assert_eq!(pages_to_read(&order, 99), order);
        assert!(pages_to_read(&order, 0).is_empty());
        assert!(pages_to_read(&[], 5).is_empty());
    }

    #[test]
    fn per_page_extraction_is_capped_and_says_it_was_cut() {
        // A bare ellipsis reads as "that is all the page said", which would let
        // the model summarize a truncated page as complete.
        let long = "x".repeat(5_000);
        let bounded = bounded_extract(&long, 1_000);
        assert!(bounded.starts_with(&"x".repeat(1_000)));
        assert!(bounded.contains("truncated at 1000 characters"));
        // Under the cap, nothing is added.
        assert_eq!(bounded_extract("  short  ", 1_000), "short");
    }

    #[test]
    fn the_corpus_budget_stops_admitting_pages_and_states_why() {
        // The cap that actually bounds the synthesis bill. A page it excludes
        // must lose its number too, or the report could cite evidence that
        // never reached the prompt.
        let mut sources = vec![
            read_source("https://a.no", &"a".repeat(3_000)),
            read_source("https://b.no", &"b".repeat(3_000)),
            read_source("https://c.no", &"c".repeat(3_000)),
        ];
        let order = read_order(&sources);
        // Budget fits the first page plus a sliver — below MIN_PAGE_CHARS, so
        // the second page is refused outright rather than admitted as a stub.
        let dropped = number_and_bound(&mut sources, &order, 3_200);

        assert_eq!(dropped, 2, "two pages were dropped for budget");
        assert_eq!(sources[0].number, Some(1));
        assert_eq!(sources[1].number, None);
        assert_eq!(sources[2].number, None);
        assert!(sources[1]
            .unread_reason
            .as_deref()
            .unwrap_or_default()
            .contains("corpus budget"));
        let admitted: usize = sources
            .iter()
            .filter_map(|source| source.extract.as_ref())
            .map(|extract| extract.chars().count())
            .sum();
        assert!(
            admitted <= 3_200 + 80,
            "admitted {admitted} chars must stay at the budget (plus the truncation notice)"
        );
    }

    #[test]
    fn cap_env_overrides_are_clamped_to_their_ceiling_and_floor() {
        // An operator typo must not turn one chat turn into an unbounded spend
        // (or into a pipeline that cannot fetch anything at all).
        assert_eq!(
            clamped_usize(Some("999"), DEFAULT_MAX_PAGES, MIN_PAGES, MAX_PAGES_CEILING),
            MAX_PAGES_CEILING
        );
        assert_eq!(
            clamped_usize(Some("0"), DEFAULT_MAX_PAGES, MIN_PAGES, MAX_PAGES_CEILING),
            MIN_PAGES
        );
        assert_eq!(
            clamped_usize(
                Some("not a number"),
                DEFAULT_MAX_PAGES,
                MIN_PAGES,
                MAX_PAGES_CEILING
            ),
            DEFAULT_MAX_PAGES
        );
        assert_eq!(clamped_usize(None, 6, 1, 10), 6);
        assert_eq!(
            clamped_u64(
                Some("99999"),
                DEFAULT_WALL_CLOCK_SECS,
                MIN_WALL_CLOCK_SECS,
                MAX_WALL_CLOCK_SECS
            ),
            MAX_WALL_CLOCK_SECS
        );
        assert_eq!(
            clamped_i32(
                Some("1"),
                DEFAULT_REPORT_TOKENS,
                MIN_REPORT_TOKENS,
                MAX_REPORT_TOKENS
            ),
            MIN_REPORT_TOKENS
        );
        // And the live accessors agree with their documented defaults when the
        // process has no override set.
        assert!((MIN_SUB_QUERIES..=MAX_SUB_QUERIES_CEILING).contains(&max_sub_queries()));
        assert!((MIN_PAGES..=MAX_PAGES_CEILING).contains(&max_pages()));
    }

    // --- honesty ----------------------------------------------------------

    #[test]
    fn coverage_states_partial_and_thin_evidence_instead_of_hiding_it() {
        // "Only 2 of 6 sub-queries returned usable sources" is the sentence
        // that has to reach the user; the report is required to reproduce it.
        let thin = Coverage {
            planned: 6,
            with_results: 2,
            search_failures: 1,
            found: 5,
            filtered: 0,
            relevance_fallback: false,
            read: 2,
            unread: 3,
            deadline_hit: true,
            corpus_exhausted: true,
        };
        let statement = coverage_statement(&thin);
        assert!(statement.contains("2 of 6 planned sub-queries"));
        assert!(statement.contains("1 search call(s) failed"));
        assert!(statement.contains("2 were read successfully and 3 could not be read"));
        assert!(statement.contains("evidence budget filled"));
        assert!(statement.contains("PARTIAL"));
        assert!(statement.contains("THIN evidence base"));

        // Zero read sources is a different, stronger statement: there is no
        // evidence base at all, and the answer must not read as researched.
        let none = Coverage {
            planned: 4,
            with_results: 0,
            search_failures: 4,
            found: 0,
            read: 0,
            unread: 0,
            ..Coverage::default()
        };
        assert!(coverage_statement(&none).contains("NO source was read successfully"));

        // A healthy run says none of that.
        let healthy = Coverage {
            planned: 5,
            with_results: 5,
            search_failures: 0,
            found: 12,
            read: 8,
            unread: 4,
            ..Coverage::default()
        };
        let healthy_statement = coverage_statement(&healthy);
        assert!(!healthy_statement.contains("THIN"));
        assert!(!healthy_statement.contains("PARTIAL"));
    }

    #[test]
    fn coverage_counts_only_sources_whose_text_we_hold_as_read() {
        let mut sources = vec![
            read_source("https://a.no", "text"),
            source("https://b.no", vec![0], 1),
            source("https://c.no", vec![0], 2),
        ];
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);
        let coverage = coverage_of(&sources, 3, 2, 1, GateReport::default(), false, false);
        assert_eq!(coverage.found, 3);
        assert_eq!(coverage.read, 1);
        assert_eq!(coverage.unread, 2);
        assert_eq!(coverage.planned, 3);
        assert_eq!(coverage.with_results, 2);
        assert_eq!(coverage.search_failures, 1);
    }

    #[test]
    fn the_synthesis_prompt_marks_unread_sources_as_uncitable() {
        // The searched-but-unread distinction has to be visible IN THE PROMPT,
        // not just in our data model — the model is what decides whether a URL
        // gets presented as a fact.
        let mut sources = vec![
            read_source("https://read.no", "the population was 720000"),
            source("https://unread.no", vec![0], 1),
        ];
        sources[1].unread_reason = Some("JavaScript-rendered".to_owned());
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);
        let coverage = coverage_of(&sources, 2, 2, 0, GateReport::default(), false, false);

        let prompt = synthesis_prompt("hvor mange bor i oslo", &sources, &coverage);
        assert!(prompt.contains("[1] Title of https://read.no — https://read.no"));
        assert!(prompt.contains("the population was 720000"));
        assert!(prompt.contains("FOUND BUT NOT READ"));
        assert!(prompt.contains("https://unread.no"));
        assert!(prompt.contains("JavaScript-rendered"));
        assert!(prompt.contains("They have NO citation number"));
        // The unread source must not appear as a numbered entry anywhere.
        assert!(!prompt.contains("[2] Title of https://unread.no"));
        // And the honesty contract is stated.
        assert!(prompt.contains("## Kunne ikke verifiseres"));
        assert!(prompt.contains("NEVER invent a source"));
        assert!(prompt.contains("2 of 2 planned sub-queries"));
    }

    #[test]
    fn a_prompt_with_no_readable_source_says_so_rather_than_listing_nothing() {
        let coverage = Coverage {
            planned: 3,
            with_results: 3,
            found: 4,
            read: 0,
            unread: 4,
            ..Coverage::default()
        };
        let sources: Vec<ResearchSource> = (0..4)
            .map(|index| source(&format!("https://x{index}.no"), vec![0], index))
            .collect();
        let prompt = synthesis_prompt("q", &sources, &coverage);
        assert!(prompt.contains("(none — no page could be read)"));
        assert!(prompt.contains("NO source was read successfully"));
    }

    // --- artifact + receipt ----------------------------------------------

    #[test]
    fn the_report_lands_as_a_versioned_document_artifact() {
        // A research report is the keepable deliverable, so it must arrive on
        // the artifact surface the "Artefakter" panel renders — kind
        // `document`, with a version the gateway assigned.
        let event = crate::artifacts::artifact_event(
            &report_artifact_id("req-42"),
            ArtifactKind::Document,
            &report_artifact_title("Hvem er Norges største fiskeeksportører?"),
            "# Rapport\n\nOslo hadde 720 000 innbyggere [1].",
            1,
        );
        let ChatEvent::Artifact {
            id,
            kind,
            title,
            content,
            version,
        } = event
        else {
            panic!("expected an artifact event");
        };
        assert_eq!(id, "research-req-42");
        assert_eq!(kind, "document");
        assert!(title.starts_with("Dyp research: Hvem er Norges"));
        assert!(content.contains("[1]"));
        assert_eq!(version, 1);
    }

    #[test]
    fn the_artifact_id_is_request_scoped_so_two_questions_are_two_documents() {
        // A thread-scoped id would file the second research question as
        // "version 2" of the first — a revision history the user cannot read.
        assert_ne!(report_artifact_id("req-1"), report_artifact_id("req-2"));
    }

    #[test]
    fn a_long_question_makes_a_bounded_title_on_a_char_boundary() {
        let title = report_artifact_title(&"æ".repeat(300));
        assert!(title.starts_with("Dyp research: "));
        assert!(title.chars().count() < 100);
        assert!(title.ends_with('\u{2026}'));
        assert_eq!(report_artifact_title("   "), "Dyp research");
    }

    #[test]
    fn the_tool_receipt_is_compact_and_carries_the_caps_it_enforced() {
        // Same pattern as tool_loop's artifact rewrite: the rich payload rides
        // the artifact/citation events, and the conversation sees only counts —
        // a receipt containing the report would be re-sent every later round.
        let coverage = Coverage {
            planned: 4,
            with_results: 3,
            search_failures: 1,
            found: 9,
            filtered: 0,
            relevance_fallback: false,
            read: 5,
            unread: 4,
            deadline_hit: false,
            corpus_exhausted: false,
        };
        let receipt = research_receipt(&["a".to_owned(), "b".to_owned()], &coverage, 12_000);
        assert_eq!(receipt["sources_found"], 9);
        assert_eq!(receipt["sources_read"], 5);
        assert_eq!(receipt["sources_unread"], 4);
        assert_eq!(receipt["search_failures"], 1);
        assert_eq!(receipt["report_chars"], 12_000);
        assert_eq!(receipt["sub_queries"].as_array().map(Vec::len), Some(2));
        assert!(receipt["caps"]["max_pages"].is_number());
        assert!(receipt["caps"]["corpus_chars"].is_number());
        // The report text itself is NOT in the receipt.
        let serialized = receipt.to_string();
        assert!(
            serialized.len() < 600,
            "receipt must stay compact: {serialized}"
        );
    }

    // --- context messages -------------------------------------------------

    #[test]
    fn the_report_context_keeps_the_citation_contract_for_the_answer_call() {
        let coverage = Coverage {
            planned: 3,
            with_results: 3,
            found: 6,
            read: 4,
            unread: 2,
            ..Coverage::default()
        };
        let message = report_context_message("q", "# Rapport\n\nFakta [1].", &coverage);
        assert!(message.contains("DEEP RESEARCH COMPLETE"));
        assert!(message.contains("Fakta [1]."));
        // Must NOT assert client UI state: the `artifacts` family is opt-in, so
        // a model told the report is "open in the panel" could point the user at
        // an empty panel.
        assert!(!message.contains("artifact panel"));
        assert!(message.contains("KEEPING every inline `[n]` marker"));
        assert!(message.contains("NO claim that is not in the report"));
        assert!(message.contains("Do not run more web searches"));
    }

    #[test]
    fn a_failed_synthesis_hands_the_real_evidence_to_the_answer_instead_of_dropping_it() {
        // The pages were fetched and paid for. Throwing them away because one
        // inference failed would turn a recoverable hiccup into an ungrounded
        // answer.
        let mut sources = vec![read_source("https://a.no", "measured 42")];
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 10_000);
        let coverage = coverage_of(&sources, 1, 1, 0, GateReport::default(), false, false);
        let message = unsynthesized_context_message(
            "q",
            &sources,
            &coverage,
            "the synthesis inference failed",
        );
        assert!(message.contains("DEEP RESEARCH PARTIAL"));
        assert!(message.contains("the synthesis inference failed"));
        assert!(message.contains("measured 42"), "the evidence is preserved");
        assert!(message.contains("[1] Title of https://a.no"));
    }

    #[test]
    fn an_unreachable_web_produces_an_explicit_failure_notice_not_a_silent_normal_answer() {
        // The user pressed a button labelled "Dyp research". If the web is
        // unreachable, the turn must SAY the research did not happen — a normal
        // ungrounded answer here is the feature lying about its own work.
        let coverage = Coverage::default();
        let message = no_evidence_context_message(
            "hvem eier Aquatiq",
            &coverage,
            "The web fetch service (Quarry edge) is not configured for this deployment, so no \
             search or page read was attempted.",
        );
        assert!(message.contains("DEEP RESEARCH FAILED"));
        assert!(message.contains("hvem eier Aquatiq"));
        assert!(message.contains("Quarry edge"));
        assert!(message.contains("nothing in your answer is web-verified"));
        assert!(message.contains("do NOT invent citations"));
        assert!(message.contains("NO source was read successfully"));
    }

    // --- upstream payload parsing ----------------------------------------

    #[test]
    fn search_output_parsing_tolerates_the_shapes_web_search_actually_returns() {
        let hits = hits_from_search_output(
            2,
            r#"[{"url":"https://a.no","title":"A","snippet":"s"},
                {"url":"  ","title":"blank"},
                {"title":"no url"},
                {"url":"https://b.no"}]"#,
        );
        assert_eq!(hits.len(), 2, "rows without a usable URL are dropped");
        assert_eq!(hits[0].sub_query, 2);
        assert_eq!(hits[0].rank, 0);
        assert_eq!(hits[1].url, "https://b.no");
        // Rank is the row's position in the provider's own ordering, so a
        // dropped row must not shift the ranks of the rows after it.
        assert_eq!(hits[1].rank, 3);
        // A non-array (an error string that slipped through) yields nothing
        // rather than panicking.
        assert!(hits_from_search_output(0, "not json").is_empty());
    }

    /// Quarry's reranker score must be picked up the moment the shared
    /// `web_search` arm starts forwarding it, and its absence must read as
    /// "not scored" rather than "scored zero" — which is the state of every
    /// deployment today, since that arm currently projects results down to
    /// `{url, title, snippet}`.
    #[test]
    fn search_output_parsing_reads_a_reranker_score_when_one_is_present() {
        let hits = hits_from_search_output(
            0,
            r#"[{"url":"https://a.no","title":"A","snippet":"s","score":0.91},
                {"url":"https://b.no","title":"B","snippet":"s"}]"#,
        );
        assert_eq!(hits[0].score, Some(0.91));
        assert_eq!(hits[1].score, None, "absent is not zero");
    }

    #[test]
    fn fetch_output_parsing_treats_blank_content_as_no_extract() {
        assert_eq!(
            extract_from_fetch_output(r#"{"final_url":"https://a.no","content":" text "}"#),
            Some("text".to_owned())
        );
        // An empty extraction is the JS-rendered-page case; it must not become
        // a citable source with no words in it.
        assert_eq!(extract_from_fetch_output(r#"{"content":"   "}"#), None);
        assert_eq!(extract_from_fetch_output("not json"), None);
    }

    #[test]
    fn empty_plan_retries_on_the_turn_model() {
        // The live failure: the cheap plan tier returns nothing while the
        // turn's model is answering fine in the same request.
        assert!(should_retry_plan_on_turn_model(
            "",
            "verevon-budget",
            "gpt-4.1"
        ));
        assert!(should_retry_plan_on_turn_model(
            "   \n ",
            "verevon-budget",
            "gpt-4.1"
        ));
    }

    #[test]
    fn a_usable_plan_is_never_retried() {
        assert!(!should_retry_plan_on_turn_model(
            "befolkning oslo 2025",
            "verevon-budget",
            "gpt-4.1"
        ));
    }

    #[test]
    fn plan_retry_needs_a_different_usable_turn_model() {
        // Same id: retrying spends the wall clock twice for one answer.
        assert!(!should_retry_plan_on_turn_model(
            "",
            "verevon-budget",
            "verevon-budget"
        ));
        // No turn model to fall back to.
        assert!(!should_retry_plan_on_turn_model("", "verevon-budget", ""));
        assert!(!should_retry_plan_on_turn_model("", "verevon-budget", "  "));
    }

    // --- the relevance gate ----------------------------------------------

    /// Build the exact source set the live weather turn produced: the four
    /// observed noise hits plus one real met.no hit.
    fn observed_weather_noise() -> Vec<ResearchSource> {
        let hits = vec![
            SearchHit {
                sub_query: 0,
                rank: 0,
                url: "https://www.instagram.com/p/Cx123/".into(),
                title: "The best cafés in Paris".into(),
                snippet: "Coffee, croissants and a corner table.".into(),
                score: None,
            },
            SearchHit {
                sub_query: 1,
                rank: 0,
                url: "https://www.fhi.no/publ/2024/skjelettalder/".into(),
                title: "Skjelettalder som metode for aldersvurdering".into(),
                snippet: "Rapport om metodens treffsikkerhet.".into(),
                score: None,
            },
            SearchHit {
                sub_query: 2,
                rank: 0,
                url: "https://www.tiktok.com/@parfyme/video/7301".into(),
                title: "Min nye parfyme".into(),
                snippet: "Denne dufter helt vilt godt.".into(),
                score: None,
            },
            SearchHit {
                sub_query: 3,
                rank: 0,
                url: "https://no.linkedin.com/in/ola-nordmann".into(),
                title: "Ola Nordmann - Senior Consultant".into(),
                snippet: "Erfaren rådgiver innen prosjektledelse.".into(),
                score: None,
            },
            SearchHit {
                sub_query: 0,
                rank: 1,
                url: "https://www.yr.no/nb/v%C3%A6rvarsel/Oslo".into(),
                title: "Været i Oslo - Yr".into(),
                snippet: "Værvarsel for Oslo time for time.".into(),
                score: None,
            },
        ];
        dedupe_hits(&hits)
    }

    /// The whole reason this gate exists. A Norwegian weather question came back
    /// with an Instagram café post, an FHI skeletal-age paper, a TikTok perfume
    /// video and a LinkedIn profile, all shown in the Kilder tab as evidence.
    /// Every one must be set aside, and the one real source must survive.
    #[test]
    fn the_gate_drops_the_observed_weather_noise_and_keeps_the_real_source() {
        let mut sources = observed_weather_noise();
        let gate = apply_relevance_gate("hva er været i Oslo i dag", &mut sources);

        assert_eq!(gate.filtered, 4, "the four observed noise hits");
        assert!(!gate.fallback_used, "a real source cleared the bar");
        let kept: Vec<&str> = sources
            .iter()
            .filter(|source| !source.filtered)
            .map(|source| source.url.as_str())
            .collect();
        assert_eq!(kept, vec!["https://www.yr.no/nb/v%C3%A6rvarsel/Oslo"]);
    }

    /// The load-bearing honesty invariant, extended to filtering: a filtered
    /// source must never end up with a citation number, even if the read phase
    /// somehow put text on it. A number is a licence to cite, and nothing the
    /// gate rejected may ever be cited.
    #[test]
    fn a_filtered_source_never_receives_a_citation_number() {
        let mut sources = observed_weather_noise();
        apply_relevance_gate("hva er været i Oslo i dag", &mut sources);
        // Adversarial: pretend the read phase managed to fetch a filtered page.
        for source in sources.iter_mut().filter(|source| source.filtered) {
            source.extract = Some("text from a page that was never about this".to_owned());
        }
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 100_000);

        for source in sources.iter().filter(|source| source.filtered) {
            assert_eq!(source.number, None, "{} was numbered", source.url);
            assert!(!source.is_read(), "{} counted as read", source.url);
        }
    }

    /// A filtered source must be excluded from the read budget, not merely from
    /// the citation list — spending a fetch on a page we already judged
    /// irrelevant burns the page cap that the real sources need.
    #[test]
    fn filtered_sources_never_enter_the_read_budget() {
        let mut sources = observed_weather_noise();
        apply_relevance_gate("hva er været i Oslo i dag", &mut sources);
        let order = read_order(&sources);
        let to_read = pages_to_read(&order, max_pages());

        assert_eq!(order.len(), 1, "only the surviving source is a read target");
        for &index in &to_read {
            assert!(!sources[index].filtered);
        }
    }

    /// A dropped hit must not silently vanish. It keeps its Kilder row, but as a
    /// `dr-unread-*` id whose snippet opens with the reason — the same vocabulary
    /// an unreadable page already used, extended rather than duplicated.
    #[test]
    fn a_filtered_source_keeps_a_labelled_kilder_row_rather_than_vanishing() {
        let mut sources = observed_weather_noise();
        apply_relevance_gate("hva er været i Oslo i dag", &mut sources);
        sources[4].extract = Some("Værvarsel for Oslo.".to_owned());
        sources[4].unread_reason = None;
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 100_000);

        let events = citation_events(&sources);
        assert_eq!(events.len(), 5, "every source is still surfaced");
        let ids: Vec<String> = events
            .iter()
            .map(|event| match event {
                ChatEvent::Citation { id, .. } => id.clone(),
                _ => unreachable!("citation_events emits only citations"),
            })
            .collect();
        assert_eq!(ids[0], "dr-1", "the one read source is the only number");
        assert!(ids[1..].iter().all(|id| id.starts_with("dr-unread-")));

        let instagram = events
            .iter()
            .find_map(|event| match event {
                ChatEvent::Citation { url, snippet, .. } if url.contains("instagram") => {
                    Some(snippet.clone())
                }
                _ => None,
            })
            .expect("the filtered instagram row is still emitted");
        assert!(
            instagram.starts_with("[not read: filtered as irrelevant"),
            "{instagram}"
        );
    }

    /// The counts the model sees must stay truthful: a filtered source is still
    /// `found`, is `unread`, is named in its own `filtered` count, and is never
    /// `read`. Reporting it any other way would trade one dishonesty for another.
    #[test]
    fn counts_stay_truthful_when_the_gate_filters() {
        let mut sources = observed_weather_noise();
        let gate = apply_relevance_gate("hva er været i Oslo i dag", &mut sources);
        sources[4].extract = Some("Værvarsel for Oslo.".to_owned());
        sources[4].unread_reason = None;
        let order = read_order(&sources);
        number_and_bound(&mut sources, &order, 100_000);
        let coverage = coverage_of(&sources, 4, 4, 0, gate, false, false);

        assert_eq!(coverage.found, 5);
        assert_eq!(coverage.filtered, 4);
        assert_eq!(coverage.read, 1);
        assert_eq!(coverage.unread, 4);
        assert_eq!(
            coverage.found,
            coverage.read + coverage.unread,
            "the arithmetic the report reproduces must add up"
        );

        let statement = coverage_statement(&coverage);
        assert!(
            statement.contains("5 unique sources were found"),
            "{statement}"
        );
        assert!(
            statement.contains("4 were set aside as irrelevant"),
            "the model must be able to say WHY 4 of 5 are unread: {statement}"
        );
    }

    /// Never filter everything away. An empty Kilder tab with no explanation is
    /// worse than a noisy one, so when nothing clears the bar the top few survive
    /// — and the coverage line says so out loud rather than passing them off as
    /// good matches.
    #[test]
    fn the_gate_never_filters_everything_away() {
        let hits: Vec<SearchHit> = (0..6)
            .map(|index| SearchHit {
                sub_query: 0,
                rank: index,
                url: format!("https://www.instagram.com/p/{index}/"),
                title: "Sommerferie i Italia".into(),
                snippet: "Bilder fra turen.".into(),
                score: None,
            })
            .collect();
        let mut sources = dedupe_hits(&hits);
        let gate = apply_relevance_gate("norsk havvind utbyggingstakt mot 2030", &mut sources);

        assert!(gate.fallback_used);
        let surviving = sources.iter().filter(|source| !source.filtered).count();
        assert_eq!(surviving, relevance::fallback_keep());
        assert!(surviving > 0, "the user must never get an empty result set");
        assert_eq!(gate.filtered, sources.len() - surviving);

        let coverage = coverage_of(&sources, 1, 1, 0, gate, false, false);
        let statement = coverage_statement(&coverage);
        assert!(
            statement.contains("no source cleared the relevance bar"),
            "a silently relaxed filter is as dishonest as a silently strict one: {statement}"
        );
    }

    /// Corroboration was actively harmful without a relevance signal in front of
    /// it: an off-topic page that every sub-query surfaced counted as several
    /// independent votes and out-ranked the one source that answered the
    /// question. Relevance tier must therefore lead the sort.
    #[test]
    fn read_order_puts_relevance_ahead_of_corroboration() {
        let mut corroborated_noise = source("https://noise.example", vec![0, 1, 2, 3], 0);
        corroborated_noise.relevance = 0.1;
        let mut lone_answer = source("https://real.no", vec![2], 5);
        lone_answer.relevance = 0.9;
        let sources = vec![corroborated_noise, lone_answer];

        assert_eq!(
            read_order(&sources),
            vec![1, 0],
            "one relevant source beats four votes for noise"
        );
    }

    /// Within one relevance tier the existing signals must still decide, so the
    /// gate refines the old ordering rather than replacing it. Scores are
    /// bucketed for exactly this reason: 0.62 does not really beat 0.61.
    #[test]
    fn read_order_still_uses_corroboration_inside_a_relevance_tier() {
        let mut low = source("https://a.no", vec![0], 0);
        low.relevance = 0.80;
        let mut high = source("https://b.no", vec![0, 1], 4);
        high.relevance = 0.99;
        let sources = vec![low, high];

        assert_eq!(
            relevance_tier(0.80),
            relevance_tier(0.99),
            "both are in the top bucket"
        );
        assert_eq!(read_order(&sources), vec![1, 0], "corroboration decides");
    }

    /// Filtered sources leave the ordering entirely — `read_order` is the single
    /// place the read budget is derived from, so exclusion has to happen here.
    #[test]
    fn read_order_excludes_filtered_sources_outright() {
        let sources = vec![
            filtered_source("https://noise.example", 0.05),
            source("https://real.no", vec![0], 0),
        ];
        assert_eq!(read_order(&sources), vec![1]);
    }

    /// An empty source set must not trip the gate's fallback: there is nothing to
    /// relax the bar for, and claiming the fallback fired would put a false
    /// statement in the coverage line.
    #[test]
    fn the_gate_is_a_no_op_on_an_empty_source_set() {
        let mut sources: Vec<ResearchSource> = Vec::new();
        let gate = apply_relevance_gate("hva er været i Oslo", &mut sources);
        assert_eq!(gate, GateReport::default());
        assert!(!gate.fallback_used);
    }
}
