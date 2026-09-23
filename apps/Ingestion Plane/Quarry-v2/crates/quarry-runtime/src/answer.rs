//! AnswerPipeline — Tavily-replacement orchestrator.
//!
//! Cycle 19 / gap-quarry cluster #18. Combines existing pieces into a
//! one-call answer flow:
//!
//! ```text
//! query
//!   ↓
//! 1. SearchProvider (Tantivy → Stract → SearXNG → Brave) — get top-K URLs
//!   ↓
//! 2. scrape_fn (caller-supplied page fetcher) — pull markdown for each URL
//!   ↓
//! 3. AiFormatRunner.query — synthesize grounded answer from concatenated sources
//!   ↓
//! returns AnswerResult { answer, citations[], sources[], usage, ... }
//! ```
//!
//! Design notes:
//!
//! - **Pluggable scrape function** — we don't take a hard dep on `PageRunner`
//!   (which lives in `pipeline.rs` and brings driver registry/security/etc.)
//!   because the answer pipeline is orthogonal to scrape mechanics. Callers
//!   wire whatever scrape impl they have. This lets the edge supply a real
//!   `PageRunner`, tests supply a mock, and future paths (e.g. a Tantivy-only
//!   short-circuit) can skip scraping entirely.
//!
//! - **ZDR propagation** — `zdr=true` flows into both the search call (free,
//!   queries are control-plane signals) and the synthesis call (Model Plane
//!   is contracted to keep ephemeral). Citations are URLs only — never raw
//!   content — so ZDR can return citations safely.
//!
//! - **Per-source markdown cap** — we truncate each source to `MAX_SOURCE_CHARS`
//!   before concatenation. Default 8K chars per source × 5 sources = 40K input
//!   tokens, well within most model context windows.
//!
//! - **Corpus write-back** — every page this pipeline fetches and extracts is
//!   submitted to the same [`LocalIndexWriteback`] hook the scrape pipeline
//!   uses, so answering a question also warms the local search tier. The
//!   submission is fire-and-forget and its rules (ZDR, tenant, body) belong to
//!   the hook, not to this file.
//!
//! - **Second extraction channel** — readability strips `script` by design, so
//!   a page that publishes its numbers as hydration state (the SSB
//!   `kommunefakta` case: the population figure lived in 29 `application/json`
//!   blobs, and 130 characters of prose survived) reads as an empty page. Every
//!   fetch therefore also runs `quarry_transform::extract_structured` over the
//!   same document, and the harvest rides the source through synthesis and out
//!   on [`Citation::structured`]. Structured facts are page content: they are
//!   returned to the caller exactly as the prose-derived answer is, and they
//!   inherit the same ZDR and tenant rules — the write-back hook decides
//!   retention, and the edge's search cache (org-keyed, skipped under ZDR)
//!   decides caching, neither of which this channel changes.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;
use quarry_transform::fingerprint::content_fingerprint;
use quarry_transform::structured::StructuredData;

use crate::ai_formats::AiFormatRunner;
use crate::local_index::TantivyLocalIndex;
use crate::pipeline::{IndexedPage, LocalIndexWriteback};
use crate::serp::{SearchOptions, SearchProvider};

/// Maximum characters of markdown to feed per source. Prevents one giant
/// page from monopolizing the synthesis context window.
const MAX_SOURCE_CHARS: usize = 8_000;

/// Default top-K search results to fetch + synthesize over.
const DEFAULT_TOP_K: usize = 5;

/// Total per-request char ceiling. If concatenated source markdown
/// exceeds this, we truncate the lowest-ranked sources first.
const MAX_TOTAL_CHARS: usize = 40_000;

/// Share of one source's character budget the harvested figures may take.
/// Deliberately small: the figures are a supplement to the prose on nearly
/// every page, and only on the pathological ones (all state, no text) is the
/// prose small enough for this to be the larger half anyway.
const MAX_FIGURE_BLOCK_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub query: String,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub zdr: Option<bool>,
    /// Tenant scope. Always populated server-side from the verified JWT
    /// claim — never trusted from the client wire. Threaded into the
    /// underlying `SearchProvider::search` so private-corpus providers
    /// (TantivyLocalIndex) restrict citations to this org's documents.
    #[serde(default)]
    pub org_id: Option<String>,
}

/// Why a source fetch produced no usable text.
///
/// Exists because the fetch used to answer only `Some`/`None`, which made
/// "the page 403'd", "the page was a PDF we couldn't read", and "the page
/// genuinely said nothing" indistinguishable — so a run where every source
/// failed looked exactly like a run that read every source, and the search
/// snippet quietly stood in for the document either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "value", rename_all = "snake_case")]
pub enum FetchFailure {
    /// Server answered, but not with 2xx.
    HttpStatus(u16),
    /// Body exceeded the fetcher's byte cap.
    TooLarge,
    /// Bytes could not be turned into text by any declared, declared-in-body,
    /// or sniffed encoding — i.e. we fetched something that isn't a document.
    Decode,
    /// Readability / PDF extraction itself failed on a body we could decode.
    Extract,
    /// Request timed out.
    Timeout,
    /// Connection-level failure that is not a timeout — DNS, TLS, refused.
    /// Kept distinct from `Timeout` because reporting a DNS failure as a
    /// timeout sends operators looking at latency instead of at resolution.
    Transport,
    /// Extraction succeeded but produced nothing — and the structured
    /// channel found no fact either. See [`FetchFailure::TooShort`].
    Empty,
    /// Extraction produced less than [`MIN_USEFUL_EXTRACTION_CHARS`] of prose
    /// *and* carried no answerable structured fact. Both halves matter: a page
    /// whose numbers live in hydration state leaves readability with a nav
    /// shell, and rejecting it on prose length alone discards the answer with
    /// the markup. See [`has_answerable_fact`].
    TooShort,
}

impl std::fmt::Display for FetchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchFailure::HttpStatus(code) => write!(f, "HTTP {code}"),
            FetchFailure::TooLarge => f.write_str("body exceeded size cap"),
            FetchFailure::Decode => f.write_str("body could not be decoded as text"),
            FetchFailure::Extract => f.write_str("content extraction failed"),
            FetchFailure::Timeout => f.write_str("request timed out"),
            FetchFailure::Transport => f.write_str("connection failed"),
            FetchFailure::Empty => f.write_str("page had no extractable content"),
            FetchFailure::TooShort => f.write_str("extracted content too short to be the page"),
        }
    }
}

/// Where a citation's text actually came from — and, when the page fetch
/// didn't supply it, why not. Additive and optional on [`Citation`] so
/// existing consumers of the JSON keep deserializing unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    /// The page body was fetched and extracted — the strong case.
    Fetched,
    /// The page fetch failed and the search-result snippet stood in for it.
    /// A consumer must treat this as materially weaker than `Fetched`: it is
    /// two provider-chosen sentences, not the document.
    SnippetFallback { reason: FetchFailure },
    /// The page fetch failed and there was no snippet to fall back to, so
    /// this source contributed nothing to synthesis.
    Failed { reason: FetchFailure },
    /// Text was available but dropped because the per-request character
    /// budget was already spent by higher-ranked sources.
    BudgetExhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    pub rank: u32,
    pub provider: String,
    /// How this source's text was obtained. `None` only for citations built
    /// by older callers that predate per-source outcome reporting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<SourceOutcome>,
    /// Machine-readable facts harvested from the page's markup, when the
    /// fetcher ran the structured channel and it found something. `None` —
    /// and therefore absent from the wire entirely — for a snippet fallback,
    /// a failed source, a fetcher with no structured channel, and any page
    /// whose harvest came back empty, which is the overwhelming majority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<StructuredData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResult {
    pub query: String,
    pub answer: String,
    pub citations: Vec<Citation>,
    pub model: String,
    pub latency_ms: u64,
    /// Number of sources actually used in synthesis (after truncation/skip).
    pub sources_used: usize,
    /// Number of sources skipped because their fetch failed.
    pub sources_skipped: usize,
    /// Of `sources_used`, how many contributed only a search snippet because
    /// their page fetch failed. Counted inside `sources_used` — they are real
    /// sources of a weaker kind, and they used to be indistinguishable from a
    /// full page read. Additive + defaulted so existing clients are unaffected.
    #[serde(default)]
    pub sources_snippet_fallback: usize,
}

/// One fetched source as both extraction channels see it.
///
/// `markdown` is what readability produced — the prose. `structured` is the
/// harvest from the markup readability strips, and is `None` both when the
/// fetcher has no structured channel at all and when the harvest was empty;
/// a consumer that has to tell those apart should look at the fetcher, not
/// at this field.
#[derive(Debug, Clone, Default)]
pub struct SourceExtraction {
    pub markdown: String,
    pub structured: Option<StructuredData>,
}

impl SourceExtraction {
    /// A prose-only extraction — what a fetcher with no structured channel
    /// produces, and what the trait's default [`MarkdownFetcher::fetch_extraction`]
    /// wraps an existing `fetch_source` into.
    pub fn prose(markdown: String) -> Self {
        Self {
            markdown,
            structured: None,
        }
    }
}

/// Whether a harvest carries a fact a question can actually be answered from.
///
/// Only [`quarry_transform::structured::KeyFigure`]s count. A raw blob is
/// "some JSON we found" and a table of nav links is not a fact, so neither may
/// rescue a page from the minimum-length floor — that would readmit exactly
/// the cookie walls and JS interstitials the floor exists to reject, since
/// nearly every page carries *some* JSON. A labelled scalar is different: it
/// is the shape an answer takes.
pub fn has_answerable_fact(data: &StructuredData) -> bool {
    data.figures
        .iter()
        .any(|f| !f.label.trim().is_empty() && !f.value.trim().is_empty())
}

/// Render the harvested figures as a labelled block for the synthesis
/// context. Empty string when there is nothing to show, so the common page
/// contributes exactly the bytes it did before this channel existed.
///
/// The block is headed and each figure keeps its unit and period, because a
/// bare "729437" is the one form of this fact nobody can check: the model has
/// to be able to say *what* was measured and *when*.
fn render_key_figures(data: &StructuredData) -> String {
    if data.figures.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n[Structured data extracted from this page]");
    for fig in &data.figures {
        let unit = fig
            .unit
            .as_deref()
            .map(|u| format!(" {u}"))
            .unwrap_or_default();
        let period = fig
            .period
            .as_deref()
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        let line = format!("\n- {}: {}{}{}", fig.label, fig.value, unit, period);
        if out.chars().count() + line.chars().count() > MAX_FIGURE_BLOCK_CHARS {
            break;
        }
        out.push_str(&line);
    }
    out
}

/// Pluggable scrape interface. The edge wires a real `PageRunner`-backed
/// impl; tests pass a closure or fake.
#[async_trait]
pub trait MarkdownFetcher: Send + Sync {
    /// Fetch the markdown body for a URL. Returns `None` on transport
    /// failure so the pipeline can skip and continue with remaining
    /// sources instead of aborting the whole answer.
    async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String>;

    /// Same fetch, but reporting *why* it failed so the pipeline can label a
    /// snippet substitution as the fallback it is instead of passing it off
    /// as the page. [`MarkdownFetcher::fetch_extraction`] is what the pipeline
    /// now calls; this remains the prose half of it.
    ///
    /// Provided (not required) so impls that only know how to say yes/no keep
    /// compiling untouched; they report [`FetchFailure::Empty`], which is the
    /// honest answer when the reason genuinely isn't known to us.
    async fn fetch_source(&self, url: &str, zdr: ZdrMode) -> Result<String, FetchFailure> {
        match self.fetch_markdown(url, zdr).await {
            Some(md) if !md.trim().is_empty() => Ok(md),
            _ => Err(FetchFailure::Empty),
        }
    }

    /// The same fetch, carrying the structured harvest alongside the prose.
    /// This is the method [`AnswerPipeline`] calls.
    ///
    /// Provided rather than required so every existing impl — the edge's
    /// wrappers, the test fakes — keeps compiling and keeps behaving exactly
    /// as it did, reporting no structured data at all rather than an empty
    /// harvest it never ran.
    async fn fetch_extraction(
        &self,
        url: &str,
        zdr: ZdrMode,
    ) -> Result<SourceExtraction, FetchFailure> {
        self.fetch_source(url, zdr)
            .await
            .map(SourceExtraction::prose)
    }
}

pub struct AnswerPipeline {
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn MarkdownFetcher>,
    formats: AiFormatRunner,
    /// Local Tantivy corpus that successful extractions are written back into.
    /// `None` disables the write-back entirely — test harnesses, and any
    /// deployment running without a local index.
    local_index: Option<TantivyLocalIndex>,
}

/// Grounded inputs for synthesis, shared by `answer()` (blocking) and the edge's
/// streaming `/v1/answer/stream` route. `combined` is empty when no source
/// contributed text (callers then return a citations-only empty answer).
pub struct PreparedAnswer {
    pub citations: Vec<Citation>,
    pub combined: String,
    pub sources_used: usize,
    pub sources_skipped: usize,
    /// See [`AnswerResult::sources_snippet_fallback`].
    pub sources_snippet_fallback: usize,
}

impl AnswerPipeline {
    pub fn new(
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn MarkdownFetcher>,
        formats: AiFormatRunner,
    ) -> Self {
        Self {
            search,
            fetcher,
            formats,
            local_index: None,
        }
    }

    /// Wire the local corpus so pages fetched while answering feed the same
    /// index the scrape path fills. Off unless set, because `new` has to stay
    /// a drop-in for the callers (and tests) that have no index at all.
    pub fn with_local_index(mut self, index: Option<TantivyLocalIndex>) -> Self {
        self.local_index = index;
        self
    }

    /// Accessor so the edge's streaming route can drive synthesis itself
    /// (`formats().query_stream(...)`) over the prepared context.
    pub fn formats(&self) -> &AiFormatRunner {
        &self.formats
    }

    /// The corpus write-back hook for one request, carrying that request's ZDR
    /// posture. Built per request rather than stored because ZDR arrives on the
    /// request while the pipeline is process-wide: a stored hook would answer
    /// with whatever posture happened to be current at construction, which on
    /// this path means persisting a ZDR tenant's pages.
    fn local_index_writeback(&self, zdr: ZdrMode) -> LocalIndexWriteback {
        LocalIndexWriteback::new(self.local_index.clone(), zdr)
    }

    /// Search + concurrent fetch + context assembly — everything up to (but not
    /// including) synthesis. Shared by `answer()` and the streaming route. The
    /// verified `org_id` flows through so private-corpus providers filter to the
    /// tenant; public-web providers ignore it.
    pub async fn prepare(&self, req: &AnswerRequest) -> QuarryResult<PreparedAnswer> {
        if req.query.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "answer query must not be empty",
            ));
        }
        let top_k = req.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, 20);
        let zdr = ZdrMode::from(req.zdr.unwrap_or(false));

        let opts = SearchOptions {
            limit: top_k as u32,
            country: req.country.clone(),
            language: req.language.clone(),
            safe_search: true,
            topic: None,
            time_range: None,
            exact_match: false,
            org_id: req.org_id.clone(),
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            // ZDR: mirrors the `zdr` this method already derives above for
            // the markdown fetcher — the search call needs the same signal
            // so a zero-retention answer request can't leak the query to
            // Brave/Serper via SmartSearchRouter.
            zdr: req.zdr.unwrap_or(false),
            // No paid-provider permission reaches this call chain:
            // `AnswerRequest` carries no `allow_paid_providers` field, and
            // absent permission means none. Fail closed rather than grant
            // blanket paid fan-out on behalf of a caller that never asked.
            allow_paid_providers: false,
            // No intent hint — the router classifies the query itself.
            intent: None,
        };
        let results = self.search.search(&req.query, &opts).await?;
        if results.is_empty() {
            return Ok(PreparedAnswer {
                citations: vec![],
                combined: String::new(),
                sources_used: 0,
                sources_skipped: 0,
                sources_snippet_fallback: 0,
            });
        }

        // Concurrent fetch of top-K sources.
        let fetch_futures = results.iter().map(|r| {
            let url = r.url.clone();
            let fetcher = self.fetcher.clone();
            async move { fetcher.fetch_extraction(&url, zdr).await }
        });
        let fetched = futures::future::join_all(fetch_futures).await;

        let mut sources_used = 0usize;
        let mut sources_skipped = 0usize;
        let mut sources_snippet_fallback = 0usize;
        let mut combined = String::with_capacity(MAX_TOTAL_CHARS);
        let mut citations: Vec<Citation> = Vec::with_capacity(results.len());

        // Corpus write-back. This pipeline fetches and extracts real pages and
        // used to throw every one of them away, so the local tier stayed thin
        // however heavily `/v1/answer` was used. All of the rules — ZDR,
        // tenant, empty body — belong to the shared hook, so this caller and
        // the scrape pipeline cannot drift into two different policies.
        let writeback = self.local_index_writeback(zdr);
        // `is_enabled` is strictly weaker than the hook's own `decide`, so this
        // only skips assembling pages that would have been dropped anyway; it
        // buys back a full-page clone and a hash per source on a user-facing
        // request that has nowhere to write.
        let writeback_enabled = writeback.is_enabled();
        let fetched_at = Utc::now();

        for (i, (result, fetched)) in results.iter().zip(fetched.iter()).enumerate() {
            // Index the body we already paid to fetch — including one the
            // character budget below will drop from synthesis, since the fetch
            // cost is sunk either way. Only the `Ok` arm: a snippet is two
            // provider-chosen sentences rather than the document, and a page
            // that failed or came back under the extraction floor is not the
            // document either.
            if let Ok(extraction) = fetched {
                let md = &extraction.markdown;
                // The trait's default `fetch_source` accepts any non-empty
                // body, so a fetcher's own length floor is not something this
                // site can assume — and a cookie wall must not reach the
                // durable corpus just because some impl handed it over as Ok.
                //
                // The floor stays on prose alone even though a page can now
                // clear the *fetch* on structured facts instead: this corpus
                // is a full-text prose index, so a page with a key figure and
                // a nav shell is still not a document anyone can retrieve.
                if writeback_enabled && md.trim().chars().count() >= MIN_USEFUL_EXTRACTION_CHARS {
                    writeback.submit(IndexedPage {
                        // The requested URL, not the post-redirect one: a
                        // `MarkdownFetcher` returns a body and nothing else, so
                        // unlike the scrape path there is no final_url to key
                        // on. It is also the URL the search tier surfaced, so
                        // the corpus key matches how the page gets looked up.
                        url: result.url.clone(),
                        title: result.title.clone(),
                        markdown: md.clone(),
                        fingerprint: content_fingerprint(md.as_bytes()).0,
                        fetched_at,
                        org_id: req.org_id.clone(),
                    });
                }
            }

            // Structured facts belong to a page we actually read. A snippet
            // fallback has no page and a failed source has nothing at all, so
            // both leave this `None` rather than inheriting a neighbour's.
            let structured = match fetched {
                Ok(extraction) => extraction.structured.clone().filter(|s| !s.is_empty()),
                Err(_) => None,
            };

            // Prefer the fetched page body; fall back to the search-result
            // snippet (paywall/block/timeout, or Data Plane chunk text). The
            // fallback is recorded rather than performed silently — a snippet
            // standing in for a blocked page is a materially weaker source,
            // and callers were previously given no way to notice.
            let (source_text, mut outcome): (Option<String>, SourceOutcome) = match fetched {
                Ok(extraction) => (Some(extraction.markdown.clone()), SourceOutcome::Fetched),
                Err(reason) => match result.snippet.clone().filter(|s| !s.trim().is_empty()) {
                    Some(snippet) => (
                        Some(snippet),
                        SourceOutcome::SnippetFallback {
                            reason: reason.clone(),
                        },
                    ),
                    None => (
                        None,
                        SourceOutcome::Failed {
                            reason: reason.clone(),
                        },
                    ),
                },
            };

            let push_citation = |outcome: SourceOutcome,
                                 structured: Option<StructuredData>,
                                 citations: &mut Vec<Citation>| {
                citations.push(Citation {
                    url: result.url.clone(),
                    title: result.title.clone(),
                    rank: (i as u32) + 1,
                    provider: result.provider.clone(),
                    outcome: Some(outcome),
                    structured,
                });
            };

            let Some(md) = source_text else {
                sources_skipped += 1;
                push_citation(outcome, structured, &mut citations);
                continue;
            };
            if combined.chars().count() >= MAX_TOTAL_CHARS {
                outcome = SourceOutcome::BudgetExhausted;
                sources_skipped += 1;
                push_citation(outcome, structured, &mut citations);
                continue;
            }
            // The harvested figures go into the synthesis context, not just
            // into the response metadata. This is the whole point of the
            // second channel: the model is what has to see "Folketallet =
            // 729 437 personer (2. kvartal 2026)" to answer with it, and on a
            // page like that one the prose says nothing.
            //
            // The facts are charged against the same per-source budget rather
            // than appended past it, so adding this channel cannot grow any
            // source's contribution beyond `MAX_SOURCE_CHARS`.
            let facts = structured
                .as_ref()
                .map(render_key_figures)
                .unwrap_or_default();
            let prose_budget = MAX_SOURCE_CHARS.saturating_sub(facts.chars().count());
            let trimmed = if md.chars().count() > prose_budget {
                md.chars().take(prose_budget).collect::<String>()
            } else {
                md.clone()
            };
            let trimmed = format!("{trimmed}{facts}");
            // Label the substitution in the synthesis context too, not just in
            // the response metadata: the model is the consumer most likely to
            // cite a two-sentence snippet as though it had read the document.
            let provenance = match &outcome {
                SourceOutcome::SnippetFallback { reason } => format!(
                    " — SEARCH SNIPPET ONLY, page fetch failed ({reason}); not the page body"
                ),
                _ => String::new(),
            };
            combined.push_str(&format!(
                "\n\n[Source {} — {}{}]\n{}",
                i + 1,
                result.url,
                provenance,
                trimmed
            ));
            if matches!(outcome, SourceOutcome::SnippetFallback { .. }) {
                sources_snippet_fallback += 1;
            }
            push_citation(outcome, structured, &mut citations);
            sources_used += 1;
        }

        Ok(PreparedAnswer {
            citations,
            combined,
            sources_used,
            sources_skipped,
            sources_snippet_fallback,
        })
    }

    pub async fn answer(&self, req: AnswerRequest) -> QuarryResult<AnswerResult> {
        let started = Instant::now();
        let prepared = self.prepare(&req).await?;
        if prepared.sources_used == 0 {
            // No source contributed text — citations-only empty answer.
            return Ok(AnswerResult {
                query: req.query,
                answer: String::new(),
                citations: prepared.citations,
                model: String::new(),
                latency_ms: started.elapsed().as_millis() as u64,
                sources_used: 0,
                sources_skipped: prepared.sources_skipped,
                sources_snippet_fallback: prepared.sources_snippet_fallback,
            });
        }
        let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
        let query_result = match req.org_id.as_deref() {
            Some(org_id) => {
                self.formats
                    .query_for_org(org_id, &prepared.combined, &req.query, zdr)
                    .await?
            }
            None => {
                self.formats
                    .query(&prepared.combined, &req.query, zdr)
                    .await?
            }
        };
        Ok(AnswerResult {
            query: req.query,
            answer: query_result.answer,
            citations: prepared.citations,
            model: query_result.model,
            latency_ms: started.elapsed().as_millis() as u64,
            sources_used: prepared.sources_used,
            sources_skipped: prepared.sources_skipped,
            sources_snippet_fallback: prepared.sources_snippet_fallback,
        })
    }
}

/// Convenience implementation that uses a closure as the fetcher. Used
/// in tests and in the edge wiring where a real PageRunner is wrapped.
pub struct ClosureFetcher<F>(pub F);

#[async_trait]
impl<F, Fut> MarkdownFetcher for ClosureFetcher<F>
where
    F: Fn(String, ZdrMode) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Option<String>> + Send,
{
    async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String> {
        (self.0)(url.to_string(), zdr).await
    }
}

/// Minimum characters of extracted text we will accept as a real source.
/// Below this, a "successful" extraction is in practice a cookie wall, a
/// JS-required interstitial, or a nav-only shell — and a ~40-character
/// readability result was being concatenated into the synthesis context and
/// cited as though it were the whole document. Failing with
/// [`FetchFailure::TooShort`] lets the pipeline fall back to the snippet and
/// say so, instead of passing off a fragment as the page.
pub const MIN_USEFUL_EXTRACTION_CHARS: usize = 400;

/// Above this share of `U+FFFD` replacement characters the "decoded" text is
/// not text at all — we fetched a binary blob, or an encoding that neither
/// the declared label nor the sniff could resolve. Feeding its mojibake into
/// synthesis would cite noise as a source, so it fails as
/// [`FetchFailure::Decode`] rather than passing through.
const MAX_REPLACEMENT_RATIO: f64 = 0.10;

/// How far in we look for the `%PDF-` marker. The header belongs at offset 0
/// per spec, but real exporters prepend junk and Acrobat itself accepts the
/// marker anywhere in the first kilobyte, so we do too.
const PDF_MAGIC_SCAN_BYTES: usize = 1024;

/// Lightweight HTTP-based markdown fetcher.
///
/// Production path for `/v1/answer`: fetches a URL via reqwest, decodes the
/// body by its declared or sniffed charset, routes PDFs through
/// `quarry-transform::pdf` and everything else through
/// `quarry-transform::readability`, and returns markdown. Skips the heavier
/// `PageRunner` machinery (driver registry, security engine, artifact store,
/// ingest, etc.) — the answer pipeline only needs raw markdown for synthesis.
/// Pages that 4xx, 5xx, time out, or yield nothing usable come back as a
/// typed [`FetchFailure`], so the pipeline continues with the remaining
/// sources *and* can report what went wrong with this one.
///
/// For full-fidelity capture (TLS impersonation, browser-rendered JS,
/// artifact-stored sources), a `PageRunnerMarkdownFetcher` is the natural
/// follow-up — same trait, swap the impl.
pub struct SimpleHttpMarkdownFetcher {
    http: reqwest::Client,
    user_agent: String,
    max_bytes: usize,
}

impl SimpleHttpMarkdownFetcher {
    /// Build with sensible defaults: 10s timeout, 5 MB body cap, the
    /// Quarry user-agent string.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            http,
            user_agent: "Quarry/2.0 (+https://triodelab.com/quarry)".into(),
            max_bytes: 5_000_000,
        }
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    pub fn with_max_bytes(mut self, n: usize) -> Self {
        self.max_bytes = n;
        self
    }
}

impl Default for SimpleHttpMarkdownFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// True when the response should be parsed as a PDF.
///
/// The Content-Type header decides when it is honest, but the magic bytes
/// have to be checked too: a large share of Norwegian public-sector document
/// links (regjeringen.no, Mattilsynet, kommune archives) are served as
/// `application/octet-stream`, or from a misconfigured file handler as
/// `text/html`, with the real type visible only in the body.
fn is_pdf(content_type: Option<&str>, bytes: &[u8]) -> bool {
    let declared = content_type
        .map(|ct| ct.to_ascii_lowercase().contains("application/pdf"))
        .unwrap_or(false);
    if declared {
        return true;
    }
    let window = &bytes[..bytes.len().min(PDF_MAGIC_SCAN_BYTES)];
    window.windows(5).any(|w| w == b"%PDF-")
}

/// Share of the decoded text that is `U+FFFD`. See [`MAX_REPLACEMENT_RATIO`].
fn replacement_ratio(text: &str) -> f64 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    text.chars().filter(|c| *c == '\u{FFFD}').count() as f64 / total as f64
}

/// Classify a reqwest error. Timeouts are called out separately because a
/// slow site and an unreachable one need different operator responses.
fn transport_failure(err: &reqwest::Error) -> FetchFailure {
    if err.is_timeout() {
        FetchFailure::Timeout
    } else {
        FetchFailure::Transport
    }
}

#[async_trait]
impl MarkdownFetcher for SimpleHttpMarkdownFetcher {
    async fn fetch_markdown(&self, url: &str, zdr: ZdrMode) -> Option<String> {
        match self.fetch_source(url, zdr).await {
            Ok(md) => Some(md),
            Err(reason) => {
                tracing::debug!(url, %reason, "fetch_markdown: no usable content");
                None
            }
        }
    }

    async fn fetch_source(&self, url: &str, zdr: ZdrMode) -> Result<String, FetchFailure> {
        self.fetch_extraction(url, zdr)
            .await
            .map(|extraction| extraction.markdown)
    }

    async fn fetch_extraction(
        &self,
        url: &str,
        _zdr: ZdrMode,
    ) -> Result<SourceExtraction, FetchFailure> {
        // 1. GET with UA + timeout.
        let resp = self
            .http
            .get(url)
            .header("user-agent", &self.user_agent)
            .send()
            .await
            .map_err(|e| transport_failure(&e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(FetchFailure::HttpStatus(status.as_u16()));
        }

        // Read Content-Type before consuming the body — both the PDF branch
        // and the charset decode need it, and the headers are gone once the
        // response is turned into bytes.
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        // Cap body size — defensive. A 50 MB SPA dump would blow up
        // synthesis even after truncation, and we have per-source caps
        // upstream anyway.
        let bytes = resp.bytes().await.map_err(|e| transport_failure(&e))?;
        if bytes.len() > self.max_bytes {
            return Err(FetchFailure::TooLarge);
        }
        if bytes.is_empty() {
            return Err(FetchFailure::Empty);
        }

        // 2. PDF or HTML. Public-sector sources publish the substance as PDF
        // far more often than as a page, so a fetcher with no PDF branch
        // reports most of that corpus as having nothing to say.
        let (text, structured) = if is_pdf(content_type.as_deref(), &bytes) {
            // No structured channel here: the harvester reads markup, and a
            // PDF has none. `None` says "not run", which is the truth.
            (
                quarry_transform::pdf::extract_text(&bytes).map_err(|_| FetchFailure::Extract)?,
                None,
            )
        } else {
            // Decode by declared charset / meta declaration / sniff. The old
            // `str::from_utf8(&bytes).ok()?` silently discarded every
            // latin-1 page — still the norm on Norwegian municipal and older
            // public-sector sites — as if it had returned no content.
            let decoded =
                quarry_transform::readability::decode_html_body(&bytes, content_type.as_deref());
            if replacement_ratio(&decoded.text) > MAX_REPLACEMENT_RATIO {
                return Err(FetchFailure::Decode);
            }
            // Two channels over one decoded document. The harvester is
            // infallible by contract — a malformed blob is skipped while the
            // rest of the page proceeds — so it cannot fail this extraction;
            // prose extraction below still stands entirely on its own.
            let structured = quarry_transform::extract_structured(&decoded.text);
            (
                quarry_transform::readability::html_to_readable_markdown(&decoded.text),
                Some(structured),
            )
        };

        // 3. Accept a source that either reads as a document or states a fact.
        //
        // Prose length alone used to decide this, which is what discarded the
        // SSB `kommunefakta` page: readability strips `script` by design, the
        // population figure lived in hydration payloads, and the 130 surviving
        // characters of nav text failed the floor — so a page we had fetched
        // successfully and could answer from was reported as having nothing to
        // say. A well-formed key figure is a usable source even when the prose
        // is a shell.
        let has_fact = structured.as_ref().is_some_and(has_answerable_fact);
        let useful_chars = text.trim().chars().count();
        if useful_chars == 0 && !has_fact {
            return Err(FetchFailure::Empty);
        }
        if useful_chars < MIN_USEFUL_EXTRACTION_CHARS && !has_fact {
            return Err(FetchFailure::TooShort);
        }
        Ok(SourceExtraction {
            markdown: text,
            structured: structured.filter(|s| !s.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp_client::ModelPlaneClient;
    use crate::serp::SearchResult;
    use quarry_transform::structured::{FigureSource, KeyFigure};
    use serde_json::json;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Mock SearchProvider that returns a canned set of results.
    struct MockSearch {
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl SearchProvider for MockSearch {
        async fn search(
            &self,
            _query: &str,
            _opts: &SearchOptions,
        ) -> QuarryResult<Vec<SearchResult>> {
            Ok(self.results.clone())
        }
        fn name(&self) -> &str {
            "mock"
        }
    }

    /// Fetcher that returns canned markdown per URL.
    struct MockFetcher {
        responses: std::collections::HashMap<String, Option<String>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl MarkdownFetcher for MockFetcher {
        async fn fetch_markdown(&self, url: &str, _zdr: ZdrMode) -> Option<String> {
            self.calls.lock().unwrap().push(url.to_string());
            self.responses.get(url).cloned().flatten()
        }
    }

    async fn pipeline_with_mp(server: &MockServer) -> AnswerPipeline {
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let search = Arc::new(MockSearch {
            results: vec![
                SearchResult {
                    url: "https://a.example/page".into(),
                    title: Some("A".into()),
                    snippet: None,
                    rank: 1,
                    provider: "mock".into(),
                    ..Default::default()
                },
                SearchResult {
                    url: "https://b.example/page".into(),
                    title: Some("B".into()),
                    snippet: None,
                    rank: 2,
                    provider: "mock".into(),
                    ..Default::default()
                },
            ],
        });
        let mut responses = std::collections::HashMap::new();
        responses.insert(
            "https://a.example/page".into(),
            Some("Rust async is great.".into()),
        );
        responses.insert(
            "https://b.example/page".into(),
            Some("Tokio is the runtime.".into()),
        );
        let fetcher = Arc::new(MockFetcher {
            responses,
            calls: Arc::new(Mutex::new(vec![])),
        });
        AnswerPipeline::new(search, fetcher, formats)
    }

    fn mp_response(content: &str) -> serde_json::Value {
        json!({
            "request_id": "req_1",
            "content": content,
            "model_used": "claude-sonnet-4-6",
        })
    }

    #[tokio::test]
    async fn end_to_end_returns_answer_with_citations() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mp_response("Rust's async ecosystem centers on Tokio.")),
            )
            .mount(&server)
            .await;

        let pipeline = pipeline_with_mp(&server).await;
        let result = pipeline
            .answer(AnswerRequest {
                query: "rust async runtime".into(),
                top_k: Some(2),
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(result.answer.contains("Tokio"));
        assert_eq!(result.citations.len(), 2);
        assert_eq!(result.citations[0].url, "https://a.example/page");
        assert_eq!(result.sources_used, 2);
        assert_eq!(result.sources_skipped, 0);
        assert!(!result.model.is_empty());
    }

    #[tokio::test]
    async fn empty_query_rejected() {
        let server = MockServer::start().await;
        let pipeline = pipeline_with_mp(&server).await;
        let err = pipeline
            .answer(AnswerRequest {
                query: "  ".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn no_search_results_returns_empty_typed_response() {
        let server = MockServer::start().await;
        let search = Arc::new(MockSearch { results: vec![] });
        let fetcher = Arc::new(MockFetcher {
            responses: Default::default(),
            calls: Arc::new(Mutex::new(vec![])),
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let pipeline = AnswerPipeline::new(search, fetcher, formats);

        let result = pipeline
            .answer(AnswerRequest {
                query: "nothing".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(result.answer.is_empty());
        assert!(result.citations.is_empty());
        assert_eq!(result.sources_used, 0);
    }

    #[tokio::test]
    async fn all_fetches_fail_returns_citations_only() {
        let server = MockServer::start().await;
        let search = Arc::new(MockSearch {
            results: vec![SearchResult {
                url: "https://broken.example/x".into(),
                title: None,
                snippet: None,
                rank: 1,
                provider: "mock".into(),
                ..Default::default()
            }],
        });
        let fetcher = Arc::new(MockFetcher {
            responses: Default::default(), // empty map → fetch_markdown returns None
            calls: Arc::new(Mutex::new(vec![])),
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let formats = AiFormatRunner::new(client);
        let pipeline = AnswerPipeline::new(search, fetcher, formats);

        let result = pipeline
            .answer(AnswerRequest {
                query: "anything".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert!(
            result.answer.is_empty(),
            "no synthesis when no fetches succeed"
        );
        assert_eq!(result.citations.len(), 1);
        assert_eq!(result.sources_used, 0);
        assert_eq!(result.sources_skipped, 1);
    }

    #[tokio::test]
    async fn closure_fetcher_works() {
        let cf =
            ClosureFetcher(
                |url: String, _zdr: ZdrMode| async move { Some(format!("mock for {url}")) },
            );
        let md = cf
            .fetch_markdown("https://x.com", ZdrMode::Off)
            .await
            .unwrap();
        assert!(md.contains("https://x.com"));
    }

    /// Body text comfortably over `MIN_USEFUL_EXTRACTION_CHARS` so the
    /// length rule isn't what a given test is measuring.
    fn long_paragraph() -> String {
        "Hello world from the article body, repeated so the extraction clears the \
         minimum-useful-length floor. "
            .repeat(6)
    }

    #[tokio::test]
    async fn simple_http_fetcher_returns_markdown_for_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/article"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "<html><body><article><h1>Rust</h1><p>{}</p></article></body></html>",
                long_paragraph()
            )))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new();
        let md = fetcher
            .fetch_markdown(&format!("{}/article", server.uri()), ZdrMode::Off)
            .await;
        assert!(md.is_some(), "should return markdown on 2xx");
        let md = md.unwrap();
        assert!(md.contains("Rust") || md.contains("article body"));
    }

    #[tokio::test]
    async fn simple_http_fetcher_returns_none_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new();
        let url = format!("{}/missing", server.uri());
        assert!(
            fetcher.fetch_markdown(&url, ZdrMode::Off).await.is_none(),
            "should return None on non-2xx"
        );
        assert_eq!(
            fetcher.fetch_source(&url, ZdrMode::Off).await,
            Err(FetchFailure::HttpStatus(404)),
            "the status must survive to the caller, not collapse into None"
        );
    }

    #[tokio::test]
    async fn simple_http_fetcher_respects_max_bytes() {
        let server = MockServer::start().await;
        let big = "x".repeat(2000);
        Mock::given(method("GET"))
            .and(path("/big"))
            .respond_with(ResponseTemplate::new(200).set_body_string(big))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new().with_max_bytes(500);
        let url = format!("{}/big", server.uri());
        assert!(
            fetcher.fetch_markdown(&url, ZdrMode::Off).await.is_none(),
            "should skip body when exceeds max_bytes"
        );
        assert_eq!(
            fetcher.fetch_source(&url, ZdrMode::Off).await,
            Err(FetchFailure::TooLarge)
        );
    }

    // --- charset decode -------------------------------------------------

    #[tokio::test]
    async fn simple_http_fetcher_decodes_latin1_page() {
        // Norwegian municipal and older public-sector sites still serve
        // ISO-8859-1. `str::from_utf8` rejected the whole body, so these
        // pages reported as "no content" rather than as text we could read.
        let server = MockServer::start().await;
        let mut body: Vec<u8> =
            b"<html><head><title>Kommune</title></head><body><article><p>".to_vec();
        for _ in 0..10 {
            // `Kafeen apner ... vaervarselet for oya` with latin-1 æ/ø/å.
            body.extend_from_slice(
                b"Kafeen \xe5pner klokken ti, og v\xe6rvarselet for \xf8ya er klart i dag. ",
            );
        }
        body.extend_from_slice(b"</p></article></body></html>");

        Mock::given(method("GET"))
            .and(path("/kommune"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body, "text/html; charset=iso-8859-1"),
            )
            .mount(&server)
            .await;

        let md = SimpleHttpMarkdownFetcher::new()
            .fetch_source(&format!("{}/kommune", server.uri()), ZdrMode::Off)
            .await
            .expect("latin-1 page must decode, not vanish");
        assert!(md.contains("åpner"), "got: {md}");
        assert!(md.contains("værvarselet"), "got: {md}");
        assert!(md.contains("øya"), "got: {md}");
    }

    #[tokio::test]
    async fn simple_http_fetcher_decodes_latin1_declared_only_in_meta() {
        let server = MockServer::start().await;
        let mut body: Vec<u8> =
            b"<html><head><meta charset=\"iso-8859-1\"></head><body><article><p>".to_vec();
        for _ in 0..10 {
            body.extend_from_slice(
                b"Kafeen \xe5pner klokken ti, og v\xe6rvarselet for \xf8ya er klart i dag. ",
            );
        }
        body.extend_from_slice(b"</p></article></body></html>");

        Mock::given(method("GET"))
            .and(path("/meta-charset"))
            .respond_with(
                ResponseTemplate::new(200)
                    // Deliberately charset-less: the document's own
                    // declaration is the only signal.
                    .set_body_raw(body, "text/html"),
            )
            .mount(&server)
            .await;

        let md = SimpleHttpMarkdownFetcher::new()
            .fetch_source(&format!("{}/meta-charset", server.uri()), ZdrMode::Off)
            .await
            .expect("meta-declared latin-1 must decode");
        assert!(md.contains("åpner"), "got: {md}");
    }

    // --- PDF branch selection -------------------------------------------

    #[test]
    fn pdf_branch_selected_by_content_type_or_magic_bytes() {
        assert!(is_pdf(Some("application/pdf"), b""));
        assert!(is_pdf(Some("application/pdf; charset=binary"), b""));
        // Served as a generic download or, from a misconfigured handler, as
        // HTML — the bytes are the only honest signal.
        assert!(is_pdf(Some("application/octet-stream"), b"%PDF-1.7\n..."));
        assert!(is_pdf(Some("text/html"), b"%PDF-1.4\n..."));
        assert!(is_pdf(None, b"%PDF-1.4\n..."));
        assert!(!is_pdf(Some("text/html"), b"<html><body>hi</body></html>"));
        assert!(!is_pdf(None, b"<html><body>hi</body></html>"));
        // The marker must be near the front, not anywhere in a long page
        // that merely talks about PDFs.
        let mut late = vec![b' '; PDF_MAGIC_SCAN_BYTES + 16];
        late.extend_from_slice(b"%PDF-1.4");
        assert!(!is_pdf(Some("text/html"), &late));
    }

    #[tokio::test]
    async fn pdf_content_type_routes_to_pdf_extraction_not_readability() {
        // An unparseable body served as a PDF must fail as `Extract`. If it
        // had gone down the HTML path instead it would have come back as
        // `TooShort`, so the reason is what proves the routing.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/doc.pdf"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(b"not really a pdf".to_vec(), "application/pdf"),
            )
            .mount(&server)
            .await;

        let err = SimpleHttpMarkdownFetcher::new()
            .fetch_source(&format!("{}/doc.pdf", server.uri()), ZdrMode::Off)
            .await
            .expect_err("unparseable PDF");
        assert_eq!(err, FetchFailure::Extract);
    }

    #[tokio::test]
    async fn pdf_magic_bytes_route_to_pdf_extraction_despite_html_content_type() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(b"%PDF-1.4 truncated garbage".to_vec(), "text/html"),
            )
            .mount(&server)
            .await;

        let err = SimpleHttpMarkdownFetcher::new()
            .fetch_source(&format!("{}/download", server.uri()), ZdrMode::Off)
            .await
            .expect_err("unparseable PDF");
        assert_eq!(
            err,
            FetchFailure::Extract,
            "magic bytes must win over a lying Content-Type"
        );
    }

    // --- minimum useful extraction --------------------------------------

    #[tokio::test]
    async fn near_empty_extraction_reported_as_too_short() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/thin"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "<html><body><article><p>We use cookies.</p></article></body></html>",
            ))
            .mount(&server)
            .await;

        let fetcher = SimpleHttpMarkdownFetcher::new();
        let url = format!("{}/thin", server.uri());
        assert_eq!(
            fetcher.fetch_source(&url, ZdrMode::Off).await,
            Err(FetchFailure::TooShort),
            "a cookie-wall-sized extraction is not the page"
        );
        assert!(fetcher.fetch_markdown(&url, ZdrMode::Off).await.is_none());
    }

    #[tokio::test]
    async fn extraction_at_or_above_floor_is_accepted() {
        let server = MockServer::start().await;
        let body = format!(
            "<html><body><article><p>{}</p></article></body></html>",
            "Substantive body text that a reader would recognise as the article. ".repeat(8)
        );
        Mock::given(method("GET"))
            .and(path("/full"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let md = SimpleHttpMarkdownFetcher::new()
            .fetch_source(&format!("{}/full", server.uri()), ZdrMode::Off)
            .await
            .expect("full article accepted");
        assert!(md.trim().chars().count() >= MIN_USEFUL_EXTRACTION_CHARS);
    }

    // --- structured-data channel ----------------------------------------

    /// The shape of the page behind the incident: the figure lives only in a
    /// hydration payload, and readability (which strips `script`) is left with
    /// a nav shell far under the length floor.
    fn hydration_only_page() -> String {
        concat!(
            "<html><body>",
            "<nav><a href=\"/s\">Statistikk</a></nav>",
            "<article><p>Kommunefakta gir noekkeltall for kommunen.</p></article>",
            "<script type=\"application/json\">",
            r#"{"keyFigureTitle":"Folketallet","number":"729 437",
               "numberDescription":"personer","time":"2. kvartal 2026"}"#,
            "</script>",
            "</body></html>"
        )
        .to_string()
    }

    async fn serve(server: &MockServer, route: &str, body: String) -> String {
        Mock::given(method("GET"))
            .and(path(route.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(server)
            .await;
        format!("{}{}", server.uri(), route)
    }

    #[tokio::test]
    async fn thin_prose_with_a_key_figure_is_not_rejected_as_too_short() {
        let server = MockServer::start().await;
        let url = serve(&server, "/kommunefakta", hydration_only_page()).await;

        let extraction = SimpleHttpMarkdownFetcher::new()
            .fetch_extraction(&url, ZdrMode::Off)
            .await
            .expect("a page that states a fact is a usable source");

        assert!(
            extraction.markdown.trim().chars().count() < MIN_USEFUL_EXTRACTION_CHARS,
            "the prose must still be under the floor — otherwise this test is \
             not exercising the rescue, got: {}",
            extraction.markdown
        );
        let figures = extraction
            .structured
            .as_ref()
            .expect("harvest present")
            .figures_labelled("folketall");
        assert_eq!(figures.len(), 1, "the incident figure must survive");
        assert_eq!(figures[0].value, "729 437");
        assert_eq!(figures[0].unit.as_deref(), Some("personer"));
        assert_eq!(figures[0].period.as_deref(), Some("2. kvartal 2026"));
    }

    #[tokio::test]
    async fn thin_prose_without_a_fact_is_still_too_short() {
        // The floor is not a blanket bypass: a page carrying JSON but no
        // labelled scalar is the cookie wall the floor exists to reject, and
        // "we found some JSON" must not readmit it.
        let server = MockServer::start().await;
        let url = serve(
            &server,
            "/cookiewall",
            "<html><body><article><p>We use cookies.</p></article>\
             <script type=\"application/json\">{\"consent\":{\"shown\":true}}</script>\
             </body></html>"
                .to_string(),
        )
        .await;

        assert_eq!(
            SimpleHttpMarkdownFetcher::new()
                .fetch_extraction(&url, ZdrMode::Off)
                .await
                .err(),
            Some(FetchFailure::TooShort)
        );
    }

    #[tokio::test]
    async fn malformed_blob_does_not_fail_the_extraction() {
        // One unparseable payload among several must cost that payload only.
        // Prose extraction is a separate channel and owes the harvest nothing.
        let server = MockServer::start().await;
        let url = serve(
            &server,
            "/mixed",
            format!(
                "<html><body><article><p>{}</p></article>\
                 <script type=\"application/json\">{{\"broken\": </script>\
                 <script type=\"application/json\">{{\"label\":\"Innbyggere\",\"value\":\"1 234\"}}</script>\
                 </body></html>",
                long_paragraph()
            ),
        )
        .await;

        let extraction = SimpleHttpMarkdownFetcher::new()
            .fetch_extraction(&url, ZdrMode::Off)
            .await
            .expect("a malformed blob must not sink the page");
        assert!(extraction.markdown.contains("article body"));
        let figures = extraction
            .structured
            .as_ref()
            .expect("harvest present")
            .figures_labelled("innbyggere");
        assert_eq!(figures.len(), 1, "the other blob still parses");
    }

    #[tokio::test]
    async fn prose_page_without_structured_data_is_byte_identical_to_before() {
        let server = MockServer::start().await;
        let url = serve(
            &server,
            "/plain",
            format!(
                "<html><body><article><p>{}</p></article></body></html>",
                long_paragraph()
            ),
        )
        .await;

        let extraction = SimpleHttpMarkdownFetcher::new()
            .fetch_extraction(&url, ZdrMode::Off)
            .await
            .expect("ordinary article");
        assert!(
            extraction.structured.is_none(),
            "an empty harvest must report as no harvest"
        );
        assert_eq!(
            SimpleHttpMarkdownFetcher::new()
                .fetch_source(&url, ZdrMode::Off)
                .await,
            Ok(extraction.markdown),
            "the prose path returns exactly what it always did"
        );
    }

    /// Fetcher that reports a harvest, to drive the pipeline without HTTP.
    struct StructuredFetcher {
        markdown: String,
        structured: StructuredData,
    }

    #[async_trait]
    impl MarkdownFetcher for StructuredFetcher {
        async fn fetch_markdown(&self, _url: &str, _zdr: ZdrMode) -> Option<String> {
            Some(self.markdown.clone())
        }
        async fn fetch_extraction(
            &self,
            _url: &str,
            _zdr: ZdrMode,
        ) -> Result<SourceExtraction, FetchFailure> {
            Ok(SourceExtraction {
                markdown: self.markdown.clone(),
                structured: Some(self.structured.clone()),
            })
        }
    }

    fn incident_figure() -> StructuredData {
        StructuredData {
            figures: vec![KeyFigure {
                label: "Folketallet".into(),
                value: "729 437".into(),
                unit: Some("personer".into()),
                period: Some("2. kvartal 2026".into()),
                source: FigureSource::Hydration,
            }],
            ..StructuredData::default()
        }
    }

    #[tokio::test]
    async fn harvested_figures_reach_synthesis_and_the_citation() {
        let server = MockServer::start().await;
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let pipeline = AnswerPipeline::new(
            one_result_search(None),
            Arc::new(StructuredFetcher {
                markdown: indexable_body(),
                structured: incident_figure(),
            }),
            AiFormatRunner::new(client),
        );

        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();

        assert!(
            prepared
                .combined
                .contains("Folketallet: 729 437 personer (2. kvartal 2026)"),
            "the model has to see the fact to answer with it, got: {}",
            prepared.combined
        );
        let wire = serde_json::to_value(&prepared.citations[0]).unwrap();
        assert_eq!(wire["structured"]["figures"][0]["value"], "729 437");
        assert_eq!(wire["structured"]["figures"][0]["source"], "hydration");
    }

    #[tokio::test]
    async fn a_source_without_a_harvest_carries_no_structured_key() {
        let server = MockServer::start().await;
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let pipeline = AnswerPipeline::new(
            one_result_search(None),
            Arc::new(FixedFetcher(indexable_body())),
            AiFormatRunner::new(client),
        );

        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();
        let wire = serde_json::to_value(&prepared.citations[0]).unwrap();
        assert!(
            wire.get("structured").is_none(),
            "an extraction with no structured data must serialize as it did before"
        );
        assert!(
            !prepared.combined.contains("[Structured data"),
            "and must not grow a heading in the synthesis context either"
        );
    }

    #[tokio::test]
    async fn figures_are_charged_against_the_per_source_budget() {
        // The facts must not be appended *past* `MAX_SOURCE_CHARS`: a page
        // that already fills the budget with prose gives some of it back.
        let server = MockServer::start().await;
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let pipeline = AnswerPipeline::new(
            one_result_search(None),
            Arc::new(StructuredFetcher {
                // A filler char that appears nowhere in the source header or
                // the rendered figure block, so the count below is exact.
                markdown: "Z".repeat(MAX_SOURCE_CHARS * 2),
                structured: incident_figure(),
            }),
            AiFormatRunner::new(client),
        );

        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();
        assert!(prepared.combined.contains("Folketallet: 729 437"));
        assert_eq!(
            prepared.combined.matches('Z').count(),
            MAX_SOURCE_CHARS - render_key_figures(&incident_figure()).chars().count(),
            "prose yields exactly the characters the figure block costs"
        );
    }

    // --- failure reasons reaching the caller ----------------------------

    /// Fetcher that always fails with a fixed reason, to drive the
    /// pipeline's snippet-fallback labelling.
    struct FailingFetcher(FetchFailure);

    #[async_trait]
    impl MarkdownFetcher for FailingFetcher {
        async fn fetch_markdown(&self, _url: &str, _zdr: ZdrMode) -> Option<String> {
            None
        }
        async fn fetch_source(&self, _url: &str, _zdr: ZdrMode) -> Result<String, FetchFailure> {
            Err(self.0.clone())
        }
    }

    #[tokio::test]
    async fn snippet_fallback_is_labelled_with_its_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("ok")))
            .mount(&server)
            .await;
        let search = Arc::new(MockSearch {
            results: vec![SearchResult {
                url: "https://blocked.example/x".into(),
                title: Some("Blocked".into()),
                snippet: Some("A two-sentence provider excerpt.".into()),
                rank: 1,
                provider: "mock".into(),
                ..Default::default()
            }],
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let pipeline = AnswerPipeline::new(
            search,
            Arc::new(FailingFetcher(FetchFailure::HttpStatus(403))),
            AiFormatRunner::new(client),
        );

        let prepared = pipeline
            .prepare(&AnswerRequest {
                query: "blocked page".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();

        assert_eq!(prepared.sources_used, 1, "snippet still feeds synthesis");
        assert_eq!(prepared.sources_snippet_fallback, 1);
        assert_eq!(
            prepared.citations[0].outcome,
            Some(SourceOutcome::SnippetFallback {
                reason: FetchFailure::HttpStatus(403)
            })
        );
        assert!(
            prepared.combined.contains("SEARCH SNIPPET ONLY"),
            "the model must be told it is reading a snippet, got: {}",
            prepared.combined
        );
        assert!(prepared.combined.contains("HTTP 403"));
    }

    #[tokio::test]
    async fn failure_without_snippet_is_reported_not_silently_dropped() {
        let server = MockServer::start().await;
        let search = Arc::new(MockSearch {
            results: vec![SearchResult {
                url: "https://timeout.example/x".into(),
                title: None,
                snippet: None,
                rank: 1,
                provider: "mock".into(),
                ..Default::default()
            }],
        });
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        let pipeline = AnswerPipeline::new(
            search,
            Arc::new(FailingFetcher(FetchFailure::Timeout)),
            AiFormatRunner::new(client),
        );

        let prepared = pipeline
            .prepare(&AnswerRequest {
                query: "slow page".into(),
                top_k: None,
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        assert_eq!(prepared.sources_used, 0);
        assert_eq!(prepared.sources_skipped, 1);
        assert_eq!(
            prepared.citations[0].outcome,
            Some(SourceOutcome::Failed {
                reason: FetchFailure::Timeout
            })
        );
    }

    #[test]
    fn citation_outcome_is_additive_on_the_wire() {
        // Older payloads have no `outcome` key; they must still deserialize,
        // and a `Fetched` citation must not grow noise for existing clients.
        let legacy: Citation = serde_json::from_value(json!({
            "url": "https://a.example/",
            "title": "A",
            "rank": 1,
            "provider": "mock",
        }))
        .expect("legacy citation still deserializes");
        assert_eq!(legacy.outcome, None);
        let wire = serde_json::to_string(&legacy).unwrap();
        assert!(!wire.contains("outcome"));
        assert!(
            !wire.contains("structured"),
            "the structured channel must be invisible on a citation that has none"
        );

        let labelled = Citation {
            outcome: Some(SourceOutcome::SnippetFallback {
                reason: FetchFailure::HttpStatus(403),
            }),
            ..legacy
        };
        let wire = serde_json::to_value(&labelled).unwrap();
        assert_eq!(wire["outcome"]["kind"], "snippet_fallback");
        assert_eq!(wire["outcome"]["reason"]["code"], "http_status");
        assert_eq!(wire["outcome"]["reason"]["value"], 403);
    }

    #[tokio::test]
    async fn top_k_clamped_to_20() {
        // top_k=999 should be capped at 20 so SearchOptions.limit doesn't
        // explode upstream providers.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mp_response("ok")))
            .mount(&server)
            .await;
        let pipeline = pipeline_with_mp(&server).await;
        let _ = pipeline
            .answer(AnswerRequest {
                query: "rust".into(),
                top_k: Some(999),
                country: None,
                language: None,
                zdr: None,
                org_id: None,
            })
            .await
            .unwrap();
        // No panic, no error — clamp worked.
    }

    // --- local corpus write-back ----------------------------------------

    /// Fetcher that hands the same body back for every URL. Doubles as the
    /// short-extraction case, because the trait's default `fetch_source`
    /// reports any non-empty body as a success.
    struct FixedFetcher(String);

    #[async_trait]
    impl MarkdownFetcher for FixedFetcher {
        async fn fetch_markdown(&self, _url: &str, _zdr: ZdrMode) -> Option<String> {
            Some(self.0.clone())
        }
    }

    /// Body comfortably over `MIN_USEFUL_EXTRACTION_CHARS`, so the length rule
    /// is never what a given corpus test is measuring.
    fn indexable_body() -> String {
        "The runtime drives futures to completion across a work-stealing scheduler. ".repeat(10)
    }

    fn one_result_search(snippet: Option<&str>) -> Arc<MockSearch> {
        Arc::new(MockSearch {
            results: vec![SearchResult {
                url: "https://example.com/tokio".into(),
                title: Some("Tokio runtime guide".into()),
                snippet: snippet.map(str::to_string),
                rank: 1,
                provider: "mock".into(),
                ..Default::default()
            }],
        })
    }

    fn corpus_request(org_id: Option<&str>, zdr: bool) -> AnswerRequest {
        AnswerRequest {
            query: "tokio runtime".into(),
            top_k: None,
            country: None,
            language: None,
            zdr: Some(zdr),
            org_id: org_id.map(str::to_string),
        }
    }

    fn corpus_pipeline(
        server: &MockServer,
        idx: &TantivyLocalIndex,
        search: Arc<MockSearch>,
        fetcher: Arc<dyn MarkdownFetcher>,
    ) -> AnswerPipeline {
        let client = Arc::new(ModelPlaneClient::new(server.uri()).unwrap());
        AnswerPipeline::new(search, fetcher, AiFormatRunner::new(client))
            .with_local_index(Some(idx.clone()))
    }

    /// Wait for the detached write-back task to land, so the assertions test
    /// the write rather than the scheduler.
    async fn await_indexed(idx: &TantivyLocalIndex, expected: u64) {
        for _ in 0..200 {
            if idx.pending_writes() >= expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!(
            "answer write-back did not reach the index: pending={}",
            idx.pending_writes()
        );
    }

    /// Long enough for a spawned write to land had one been submitted, so a
    /// "nothing was indexed" assertion cannot pass merely by winning a race.
    async fn settle() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn answered_pages_reach_the_local_corpus() {
        let server = MockServer::start().await;
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let pipeline = corpus_pipeline(
            &server,
            &idx,
            one_result_search(None),
            Arc::new(FixedFetcher(indexable_body())),
        );

        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();
        assert_eq!(prepared.sources_used, 1);

        await_indexed(&idx, 1).await;
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 1);

        let hits = idx
            .search(
                "work-stealing scheduler",
                &SearchOptions {
                    org_id: Some("org_a".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(hits.len(), 1, "the page must come back for its own tenant");
        assert!(hits[0].url.contains("/tokio"));
    }

    #[tokio::test]
    async fn zdr_answer_persists_no_page_in_the_corpus() {
        let server = MockServer::start().await;
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let pipeline = corpus_pipeline(
            &server,
            &idx,
            one_result_search(None),
            Arc::new(FixedFetcher(indexable_body())),
        );

        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), true))
            .await
            .unwrap();
        assert_eq!(
            prepared.sources_used, 1,
            "ZDR still answers from the fetched page"
        );

        settle().await;
        assert_eq!(idx.pending_writes(), 0);
        idx.flush().await.unwrap();
        assert_eq!(
            idx.doc_count(),
            0,
            "a ZDR tenant's content must leave no durable trace"
        );
    }

    #[tokio::test]
    async fn answer_without_a_verified_tenant_indexes_nothing() {
        let server = MockServer::start().await;
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let pipeline = corpus_pipeline(
            &server,
            &idx,
            one_result_search(None),
            Arc::new(FixedFetcher(indexable_body())),
        );

        let prepared = pipeline
            .prepare(&corpus_request(None, false))
            .await
            .unwrap();
        assert_eq!(
            prepared.sources_used, 1,
            "an untenanted request still gets its answer"
        );

        settle().await;
        assert_eq!(idx.pending_writes(), 0);
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 0);

        // Nor reachable by an unscoped read: storing the page under an
        // empty-string tenant is the cross-org leak this guard exists for.
        let unscoped = idx
            .search_all_orgs("work-stealing scheduler", &SearchOptions::default())
            .await
            .unwrap();
        assert!(unscoped.is_empty(), "leaked into the shared corpus");
    }

    #[tokio::test]
    async fn failed_and_too_short_extractions_stay_out_of_the_corpus() {
        let server = MockServer::start().await;

        // The fetch failed and the search snippet stood in for it. The snippet
        // still feeds synthesis, but it is two provider-chosen sentences — it
        // must not be stored as though it were the document.
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let pipeline = corpus_pipeline(
            &server,
            &idx,
            one_result_search(Some("A two-sentence provider excerpt.")),
            Arc::new(FailingFetcher(FetchFailure::TooShort)),
        );
        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();
        assert_eq!(prepared.sources_snippet_fallback, 1);
        settle().await;
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 0, "a snippet is not the page");

        // A fetcher reporting success on a cookie-wall-sized body: the default
        // `fetch_source` only checks non-empty, so the extraction floor has to
        // hold at the write-back site too.
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let pipeline = corpus_pipeline(
            &server,
            &idx,
            one_result_search(None),
            Arc::new(FixedFetcher("We use cookies.".into())),
        );
        let prepared = pipeline
            .prepare(&corpus_request(Some("org_a"), false))
            .await
            .unwrap();
        assert_eq!(
            prepared.sources_used, 1,
            "short text is still offered to synthesis"
        );
        settle().await;
        idx.flush().await.unwrap();
        assert_eq!(
            idx.doc_count(),
            0,
            "an under-floor extraction is not the document"
        );
    }
}
