//! Tantivy-backed local search index over Quarry's own scraped corpus.
//!
//! Cycle 19, gap-quarry cluster #16. Implements [`SearchProvider`] so it
//! slots into [`FallbackSearchProvider`] at the front of the chain — every
//! `/v1/search` query hits the local corpus first (microseconds), only
//! falling through to Stract/SearXNG/Brave on miss.
//!
//! ## Indexing model
//!
//! Each scraped page becomes one Tantivy document with the schema:
//!
//! | Field | Type | Indexing |
//! |---|---|---|
//! | `url` | `STRING` | stored, un-tokenized — also the primary key |
//! | `title` | `TEXT` | stored, tokenized, boosted in queries |
//! | `body` | `TEXT` | tokenized (not stored — fetch markdown via artifact ref) |
//! | `host` | `STRING` | indexed exact, stored — for facet filters |
//! | `org_id` | `STRING` | indexed exact, stored — multi-tenant scoping |
//! | `fingerprint` | `STRING` | stored — change-detection link-back |
//! | `fetched_at` | `DATE` | stored, indexed — recency boost / time filters |
//!
//! ## Org scoping
//!
//! Tenant isolation is enforced at BOTH ends, because either end alone is
//! a leak:
//!
//! - **Write.** [`TantivyLocalIndex::add_document`] refuses a document whose
//!   `org_id` is empty. There is no such tenant, so a document stored under
//!   `""` belongs to nobody — and a document belonging to nobody is a
//!   document belonging to whoever asks next. The write-back callers skip
//!   such pages entirely rather than inventing a placeholder tenant.
//! - **Read.** A query with no org is not "search everything"; it is
//!   "caller failed to say who it is". [`TantivyLocalIndex::search_with_org`]
//!   answers it with zero results. An operator read that genuinely wants
//!   every tenant has to say so out loud via
//!   [`TantivyLocalIndex::search_all_orgs`], which no request-path code
//!   calls.
//!
//! Before both guards existed, an org-less scrape was stored under `""` and
//! an org-less query ran unfiltered, so the two defects composed into a
//! cross-tenant read. Removing either guard reopens it.
//!
//! ## Persistence
//!
//! - In-memory mode: `TantivyLocalIndex::in_memory()` — tests, dev
//! - On-disk mode: `TantivyLocalIndex::open(path)` — production
//!
//! Writer is `Arc<Mutex<IndexWriter>>` so multiple `add_document` calls
//! from concurrent PageRunner success paths serialize cheaply. `flush`
//! commits and reloads the reader; tests call it directly, and the edge
//! runs it on a ticker (see `quarry-edge/src/main.rs`) so an unclean exit
//! loses at most one commit interval. Relying on Tantivy's autocommit alone
//! meant a restart could silently drop every write since the last
//! heap-pressure flush.
//!
//! ## Retention
//!
//! The corpus is a cache of pages we happened to fetch, not a system of
//! record — nothing downstream reconstructs state from it, so bounding it
//! is safe. [`TantivyLocalIndex::enforce_retention`] drops documents past a
//! maximum age and, if still over a document ceiling, evicts oldest-first.
//! Both bounds are env-tunable; see [`RETENTION_DAYS_ENV`] / [`MAX_DOCS_ENV`].

use std::ops::Bound;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, BooleanQuery, Occur, Query, QueryParser, RangeQuery, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, FAST, INDEXED, STORED, STRING, TEXT};
use tantivy::time::OffsetDateTime;
use tantivy::{
    doc, DateTime as TantivyDate, DocAddress, Index, IndexReader, IndexWriter, Order, ReloadPolicy,
    Term,
};
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::serp::{SearchOptions, SearchProvider, SearchResult};

/// Tantivy writer heap budget (50 MB). Plenty for batch ingestion at our
/// expected per-scrape document size (~few KB titles + tokenized body).
const WRITER_HEAP_BYTES: usize = 50_000_000;

/// Maximum age of an indexed document, in days. Override with
/// [`RETENTION_DAYS_ENV`]; `0` disables age-based eviction entirely.
///
/// 90 days is chosen against what the corpus is *for*: it is the warm first
/// tier of the search router, and a page we last saw a quarter ago is no
/// longer evidence about that URL — the live fetch behind the SERP tiers is
/// both fresher and cheap. Keeping it longer trades index size and merge
/// cost for answers we would rather not serve.
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Hard ceiling on indexed documents. Override with [`MAX_DOCS_ENV`]; `0`
/// disables the ceiling.
///
/// The age bound alone does not bound size: a heavy crawl can add millions
/// of pages inside the retention window, and the edge runs with the index on
/// the container's own disk. One million documents is roughly a few GB at
/// our per-page body size — large enough that the ceiling never fires for
/// normal product use, small enough that a runaway crawl cannot fill the
/// volume and take the whole edge down with it.
const DEFAULT_MAX_DOCS: u64 = 1_000_000;

/// Env knob for [`DEFAULT_RETENTION_DAYS`].
pub const RETENTION_DAYS_ENV: &str = "QUARRY_LOCAL_INDEX_RETENTION_DAYS";
/// Env knob for [`DEFAULT_MAX_DOCS`].
pub const MAX_DOCS_ENV: &str = "QUARRY_LOCAL_INDEX_MAX_DOCS";

/// Age bound in days, or `0` when disabled. Same read-at-use-site,
/// parse-or-default convention as `QUARRY_BROWSER_SETTLE_MS` — a malformed
/// value falls back to the default rather than failing the process, because
/// a typo in a tuning knob must not stop the edge from booting.
fn retention_days() -> i64 {
    std::env::var(RETENTION_DAYS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|d| *d >= 0)
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

/// Document ceiling, or `0` when disabled. See [`retention_days`].
fn max_documents() -> u64 {
    std::env::var(MAX_DOCS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_DOCS)
}

/// What one [`TantivyLocalIndex::enforce_retention`] pass did. Returned
/// rather than only logged so the caller can emit it as telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionOutcome {
    /// Whether an age-based delete was issued this pass. Tantivy applies
    /// `delete_query` lazily at merge time and reports no match count, so
    /// there is no honest number to return here.
    pub age_pass_ran: bool,
    /// Documents evicted to get back under the ceiling.
    pub evicted_by_ceiling: u64,
    /// `doc_count()` after the pass.
    pub docs_after: u64,
}

/// One indexed scraped page.
#[derive(Debug, Clone)]
pub struct LocalDocument {
    pub url: String,
    pub title: String,
    pub body: String,
    pub host: String,
    pub org_id: String,
    pub fingerprint: String,
    pub fetched_at: DateTime<Utc>,
}

/// Embedded BM25 index over Quarry's scraped corpus.
///
/// Cheap to clone — internally `Arc<...>`. Safe to share across PageRunner
/// instances and `/v1/search` handlers.
#[derive(Clone)]
pub struct TantivyLocalIndex {
    schema: Arc<IndexSchema>,
    index: Index,
    reader: IndexReader,
    writer: Arc<Mutex<IndexWriter>>,
    /// Documents added since the last successful commit. The edge's commit
    /// ticker reads this to flush on volume as well as on time, so a burst
    /// crawl doesn't sit uncommitted for a full interval.
    pending: Arc<AtomicU64>,
}

struct IndexSchema {
    url: Field,
    title: Field,
    body: Field,
    host: Field,
    org_id: Field,
    fingerprint: Field,
    fetched_at: Field,
    inner: Schema,
}

impl IndexSchema {
    fn build() -> Self {
        let mut sb = Schema::builder();
        // URL: STRING (exact-match, un-tokenized) so `delete_term` for a
        // full URL works as a primary-key delete. Tokenizing URLs would
        // break upsert semantics — `delete_term` only matches token-level
        // terms, so a tokenized field can't be deleted by the full URL.
        let url = sb.add_text_field("url", STRING | STORED);
        // Title gets the same TEXT treatment but we query it with a
        // higher boost via field_boosts.
        let title = sb.add_text_field("title", TEXT | STORED);
        // Body — tokenized but NOT stored. Storing markdown would inflate
        // the index by 100x; callers re-fetch markdown via artifact_ref
        // anyway.
        let body = sb.add_text_field("body", TEXT);
        // Host + org_id: exact-match string facets.
        let host = sb.add_text_field("host", STRING | STORED);
        let org_id = sb.add_text_field("org_id", STRING | STORED);
        let fingerprint = sb.add_text_field("fingerprint", STRING | STORED);
        // fetched_at: indexed + FAST so we can filter and sort by recency.
        let fetched_at = sb.add_date_field("fetched_at", STORED | INDEXED | FAST);
        Self {
            url,
            title,
            body,
            host,
            org_id,
            fingerprint,
            fetched_at,
            inner: sb.build(),
        }
    }
}

impl TantivyLocalIndex {
    /// In-memory variant — used by tests, dev, and ephemeral runs. Drops
    /// state on process exit.
    pub fn in_memory() -> QuarryResult<Self> {
        let schema = Arc::new(IndexSchema::build());
        let index = Index::create_in_ram(schema.inner.clone());
        Self::from_index(schema, index)
    }

    /// On-disk variant — production. Creates the directory if absent;
    /// otherwise opens the existing index in-place.
    pub fn open(path: impl AsRef<Path>) -> QuarryResult<Self> {
        let schema = Arc::new(IndexSchema::build());
        let path = path.as_ref();
        std::fs::create_dir_all(path).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: failed to create {}: {e}", path.display()),
            )
        })?;
        let dir = tantivy::directory::MmapDirectory::open(path).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: mmap open {} failed: {e}", path.display()),
            )
        })?;
        let index = Index::open_or_create(dir, schema.inner.clone()).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: open_or_create failed: {e}"),
            )
        })?;
        Self::from_index(schema, index)
    }

    fn from_index(schema: Arc<IndexSchema>, index: Index) -> QuarryResult<Self> {
        let writer = index.writer(WRITER_HEAP_BYTES).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: writer init failed: {e}"),
            )
        })?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: reader init failed: {e}"),
                )
            })?;
        Ok(Self {
            schema,
            index,
            reader,
            writer: Arc::new(Mutex::new(writer)),
            pending: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Add (or replace) a document.
    ///
    /// Replacement semantics: deletes any prior doc with the same `url`
    /// before adding the new one. This makes the index converge to "latest
    /// fetched copy of URL X" without unbounded growth on re-crawls.
    ///
    /// Rejects a document with an empty `org_id`. This is the write half of
    /// the tenant guard described in the module docs: an untenanted document
    /// is readable by any query that also fails to name a tenant, so there
    /// is no safe way to store one. Callers are fire-and-forget and log the
    /// error, so a mis-wired caller is loud without being fatal.
    pub async fn add_document(&self, doc: LocalDocument) -> QuarryResult<()> {
        if doc.org_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                format!(
                    "local_index: refusing to index {} with no org_id (untenanted documents leak across tenants)",
                    doc.url
                ),
            ));
        }
        let writer = self.writer.lock().await;
        // Delete prior copies of the same URL.
        let url_term = Term::from_field_text(self.schema.url, &doc.url);
        writer.delete_term(url_term);

        let ts = TantivyDate::from_utc(
            OffsetDateTime::from_unix_timestamp(doc.fetched_at.timestamp())
                .unwrap_or(OffsetDateTime::UNIX_EPOCH),
        );

        let _ = writer
            .add_document(doc!(
                self.schema.url => doc.url.clone(),
                self.schema.title => doc.title.clone(),
                self.schema.body => doc.body.clone(),
                self.schema.host => doc.host.clone(),
                self.schema.org_id => doc.org_id.clone(),
                self.schema.fingerprint => doc.fingerprint.clone(),
                self.schema.fetched_at => ts,
            ))
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: add_document failed: {e}"),
                )
            })?;
        self.pending.fetch_add(1, AtomicOrdering::Relaxed);
        Ok(())
    }

    /// Documents added since the last successful [`Self::flush`]. Drives the
    /// edge's "commit every N docs" trigger.
    pub fn pending_writes(&self) -> u64 {
        self.pending.load(AtomicOrdering::Relaxed)
    }

    /// Commit pending writes and reload the reader, making everything added
    /// so far visible to the next search and durable across a restart.
    ///
    /// Tantivy's autocommit is driven by writer-heap pressure, not by time,
    /// so at our document size a low-traffic edge could hold hours of writes
    /// in memory and lose all of them on a restart. The edge therefore calls
    /// this on a ticker and once on shutdown; tests call it directly.
    pub async fn flush(&self) -> QuarryResult<()> {
        let mut writer = self.writer.lock().await;
        // Read the counter under the writer lock and subtract (rather than
        // storing 0) after the commit. Reading it before the lock would let
        // two concurrent flushes both observe the same N and both subtract it,
        // wrapping the counter past zero; subtracting rather than zeroing
        // keeps `add_document` calls that land mid-commit counted for the next
        // flush instead of silently forgetting them.
        let observed = self.pending.load(AtomicOrdering::Relaxed);
        writer.commit().map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: commit failed: {e}"),
            )
        })?;
        self.pending.fetch_sub(observed, AtomicOrdering::Relaxed);
        self.reader.reload().map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: reader reload failed: {e}"),
            )
        })?;
        Ok(())
    }

    /// Bound the corpus by age and by document count, using the env-configured
    /// bounds ([`RETENTION_DAYS_ENV`] / [`MAX_DOCS_ENV`]). Safe to call
    /// repeatedly; a pass with nothing to do is cheap.
    pub async fn enforce_retention(&self) -> QuarryResult<RetentionOutcome> {
        self.enforce_retention_with(retention_days(), max_documents())
            .await
    }

    /// [`Self::enforce_retention`] with the bounds passed explicitly: `days`
    /// and `ceiling` of `0` each disable their pass.
    ///
    /// Split out from the env read so tests can drive both bounds
    /// deterministically — env vars are process-global and these tests run
    /// in parallel with every other test in the crate.
    ///
    /// Runs the age bound first because it is a single `delete_query` over
    /// the whole index and usually removes enough on its own — the ceiling
    /// pass, which has to read the oldest documents back to get their URLs,
    /// then has less to do or nothing at all.
    pub async fn enforce_retention_with(
        &self,
        days: i64,
        ceiling: u64,
    ) -> QuarryResult<RetentionOutcome> {
        let mut outcome = RetentionOutcome::default();

        if days > 0 {
            let cutoff_secs = Utc::now().timestamp().saturating_sub(days * 86_400);
            let cutoff = TantivyDate::from_utc(
                OffsetDateTime::from_unix_timestamp(cutoff_secs)
                    .unwrap_or(OffsetDateTime::UNIX_EPOCH),
            );
            let stale = RangeQuery::new(
                Bound::Unbounded,
                Bound::Excluded(Term::from_field_date(self.schema.fetched_at, cutoff)),
            );
            {
                let writer = self.writer.lock().await;
                writer.delete_query(Box::new(stale)).map_err(|e| {
                    QuarryError::new(
                        ErrorCode::Internal,
                        format!("local_index: retention delete_query failed: {e}"),
                    )
                })?;
            }
            // Commit before measuring: `doc_count()` reads the reader, which
            // only sees committed state, so the ceiling pass below would
            // otherwise size itself against documents this pass just deleted.
            self.flush().await?;
            outcome.age_pass_ran = true;
        }

        if ceiling > 0 {
            let current = self.doc_count();
            if current > ceiling {
                let excess = (current - ceiling) as usize;
                let urls = self.oldest_urls(excess)?;
                if !urls.is_empty() {
                    {
                        let writer = self.writer.lock().await;
                        for url in &urls {
                            writer.delete_term(Term::from_field_text(self.schema.url, url));
                        }
                    }
                    outcome.evicted_by_ceiling = urls.len() as u64;
                    self.flush().await?;
                }
            }
        }

        outcome.docs_after = self.doc_count();
        Ok(outcome)
    }

    /// URLs of the `n` oldest documents by `fetched_at`, oldest first.
    ///
    /// Eviction goes through the URL term rather than the doc address
    /// because `delete_term` is the only deletion Tantivy offers that
    /// survives the segment merge a `DocAddress` does not.
    fn oldest_urls(&self, n: usize) -> QuarryResult<Vec<String>> {
        if n == 0 {
            return Ok(Vec::new());
        }
        let searcher = self.reader.searcher();
        let collector =
            TopDocs::with_limit(n).order_by_fast_field::<TantivyDate>("fetched_at", Order::Asc);
        let hits: Vec<(Option<TantivyDate>, DocAddress)> =
            searcher.search(&AllQuery, &collector).map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: oldest-document scan failed: {e}"),
                )
            })?;
        let mut urls = Vec::with_capacity(hits.len());
        for (_fetched_at, address) in hits {
            let retrieved: tantivy::TantivyDocument = searcher.doc(address).map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: doc fetch failed during eviction: {e}"),
                )
            })?;
            if let Some(url) = field_text(&retrieved, self.schema.url) {
                urls.push(url);
            }
        }
        Ok(urls)
    }

    /// Query one tenant's documents. Combines the user query (title+body
    /// weighted) with an `org_id` filter. Returns up to `limit` results,
    /// ranked by BM25 score with a recency nudge.
    ///
    /// **An absent or empty `org_id` returns nothing.** It is not a request
    /// to search every tenant — it is a caller that failed to say who it is,
    /// and the only answer that cannot leak is none. Production callers
    /// (`/v1/search`, `/v1/answer`, `/v1/answer/stream`) all thread the
    /// verified JWT `org_id` through `SearchOptions`, so this branch means a
    /// bug, not a legitimate mode; it logs at warn for that reason. An
    /// operator read that really does want every tenant calls
    /// [`Self::search_all_orgs`] instead.
    pub async fn search_with_org(
        &self,
        query: &str,
        org_id: Option<&str>,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let Some(org) = org_id.map(str::trim).filter(|o| !o.is_empty()) else {
            tracing::warn!(
                provider = "tantivy_local",
                "local index queried with no org scope; returning no results (see search_all_orgs for operator reads)"
            );
            return Ok(Vec::new());
        };
        self.search_scoped(query, Some(org), opts).await
    }

    /// Query across EVERY tenant. Operator and diagnostic use only — health
    /// checks, index inspection, corpus statistics.
    ///
    /// Deliberately not reachable from the request path and deliberately
    /// named for what it does: the org filter is the only thing separating
    /// tenants in a shared index, and a function that drops it should be
    /// impossible to call by accident or by forgetting an argument.
    pub async fn search_all_orgs(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        self.search_scoped(query, None, opts).await
    }

    /// Shared query execution. `org_filter: None` means genuinely unfiltered
    /// — only [`Self::search_all_orgs`] passes it.
    async fn search_scoped(
        &self,
        query: &str,
        org_filter: Option<&str>,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let searcher = self.reader.searcher();
        let limit = opts.limit.max(1) as usize;

        // Boost title 3x vs body — same convention as the SourceTraceBuilder.
        let mut parser =
            QueryParser::for_index(&self.index, vec![self.schema.title, self.schema.body]);
        parser.set_field_boost(self.schema.title, 3.0);
        let user_query = parser.parse_query_lenient(query).0;

        let final_query: Box<dyn Query> = match org_filter {
            Some(o) => {
                let org_query = TermQuery::new(
                    Term::from_field_text(self.schema.org_id, o),
                    IndexRecordOption::Basic,
                );
                Box::new(BooleanQuery::new(vec![
                    (Occur::Must, user_query),
                    (Occur::Must, Box::new(org_query)),
                ]))
            }
            None => user_query,
        };

        // Over-fetch BM25 candidates so the recency rerank below has room to
        // reorder before truncating to the caller's limit.
        let candidate_limit = limit
            .saturating_mul(RECENCY_OVERFETCH)
            .clamp(limit, RECENCY_MAX_CANDIDATES);
        let top_docs = searcher
            .search(
                &*final_query,
                &TopDocs::with_limit(candidate_limit).order_by_score(),
            )
            .map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: search failed: {e}"),
                )
            })?;

        // Combine BM25 with a gentle recency decay over the indexed `fetched_at`
        // so fresher corpus documents edge ahead when relevance is comparable —
        // without letting age override a strong lexical match (the multiplier is
        // bounded to `[RECENCY_FLOOR, 1.0]`).
        let now_secs = Utc::now().timestamp();
        let mut scored: Vec<(f32, SearchResult)> = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved: tantivy::TantivyDocument = searcher.doc(doc_address).map_err(|e| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("local_index: doc fetch failed: {e}"),
                )
            })?;
            let url = field_text(&retrieved, self.schema.url).unwrap_or_default();
            let title = field_text(&retrieved, self.schema.title);
            let recency = recency_multiplier(
                field_date_secs(&retrieved, self.schema.fetched_at),
                now_secs,
            );
            scored.push((
                score * recency,
                SearchResult {
                    url,
                    title,
                    snippet: None,
                    rank: 0,
                    provider: "tantivy_local".to_string(),
                    ..Default::default()
                },
            ));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        let results = scored
            .into_iter()
            .enumerate()
            .map(|(idx, (_combined, mut result))| {
                result.rank = (idx as u32) + 1;
                result
            })
            .collect();
        Ok(results)
    }

    /// Document count for telemetry/health endpoints. Cheap.
    pub fn doc_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }
}

fn field_text(doc: &tantivy::TantivyDocument, field: Field) -> Option<String> {
    use tantivy::schema::Value;
    doc.get_first(field)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Over-fetch factor + hard cap on BM25 candidates considered for the recency
/// rerank, so a small `limit` still has neighbours to reorder.
const RECENCY_OVERFETCH: usize = 4;
const RECENCY_MAX_CANDIDATES: usize = 200;
/// Recency decay knobs: a `fetched_at` half-life and a floor so age only nudges
/// ranking (the multiplier stays within `[RECENCY_FLOOR, 1.0]`).
const RECENCY_HALF_LIFE_DAYS: f32 = 180.0;
const RECENCY_FLOOR: f32 = 0.5;

/// Read a stored DATE field as unix seconds.
fn field_date_secs(doc: &tantivy::TantivyDocument, field: Field) -> Option<i64> {
    use tantivy::schema::Value;
    doc.get_first(field)
        .and_then(|v| v.as_datetime())
        .map(|dt| dt.into_utc().unix_timestamp())
}

/// Gentle recency multiplier in `[RECENCY_FLOOR, 1.0]`: 1.0 for a just-fetched
/// doc, decaying by half-life toward the floor for older docs. A missing date is
/// neutral (1.0).
fn recency_multiplier(fetched_at_secs: Option<i64>, now_secs: i64) -> f32 {
    let Some(ts) = fetched_at_secs else {
        return 1.0;
    };
    let age_days = (now_secs.saturating_sub(ts).max(0) as f32) / 86_400.0;
    let decay = 0.5_f32.powf(age_days / RECENCY_HALF_LIFE_DAYS);
    RECENCY_FLOOR + (1.0 - RECENCY_FLOOR) * decay
}

#[cfg(test)]
mod recency_tests {
    use super::{recency_multiplier, RECENCY_FLOOR, RECENCY_HALF_LIFE_DAYS};

    const DAY: i64 = 86_400;

    #[test]
    fn fresh_doc_is_unboosted_and_old_doc_decays() {
        let now = 1_000 * DAY;
        // Just-fetched → ~1.0 (top of the range).
        assert!((recency_multiplier(Some(now), now) - 1.0).abs() < 1e-4);
        // One half-life old → floor + half the remaining range.
        let half_life = now - (RECENCY_HALF_LIFE_DAYS as i64) * DAY;
        let expected = RECENCY_FLOOR + (1.0 - RECENCY_FLOOR) * 0.5;
        assert!((recency_multiplier(Some(half_life), now) - expected).abs() < 1e-2);
        // Ancient doc → approaches the floor, never below it.
        let ancient = now - 5_000 * DAY;
        let m = recency_multiplier(Some(ancient), now);
        assert!((RECENCY_FLOOR..RECENCY_FLOOR + 0.05).contains(&m));
    }

    #[test]
    fn missing_or_future_date_is_neutral() {
        let now = 1_000 * DAY;
        assert_eq!(recency_multiplier(None, now), 1.0);
        // Future fetched_at (clock skew) clamps to age 0 → ~1.0, never > 1.0.
        let future = recency_multiplier(Some(now + 10 * DAY), now);
        assert!((future - 1.0).abs() < 1e-4);
    }
}

#[async_trait]
impl SearchProvider for TantivyLocalIndex {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        // Tenant isolation: results are restricted to `SearchOptions.org_id`.
        // An unset org_id yields NO results rather than an unfiltered read —
        // see `search_with_org`. This matters most here, because this impl is
        // what `SmartSearchRouter` registers as the first search tier, so any
        // caller anywhere that builds `SearchOptions` without an org reaches
        // the corpus through this one line.
        self.search_with_org(query, opts.org_id.as_deref(), opts)
            .await
    }

    fn name(&self) -> &str {
        "tantivy_local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const DAY_SECS: i64 = 86_400;

    fn doc(url: &str, title: &str, body: &str, org: &str) -> LocalDocument {
        doc_aged(url, title, body, org, 0)
    }

    /// Same as [`doc`] but with `fetched_at` pushed `age_days` into the past,
    /// so retention tests can build a corpus with a known age ordering.
    fn doc_aged(url: &str, title: &str, body: &str, org: &str, age_days: i64) -> LocalDocument {
        LocalDocument {
            url: url.into(),
            title: title.into(),
            body: body.into(),
            host: url.split('/').nth(2).unwrap_or("example.com").to_string(),
            org_id: org.into(),
            fingerprint: format!("blake3:{}", url),
            fetched_at: Utc::now() - chrono::Duration::seconds(age_days * DAY_SECS),
        }
    }

    /// Search options scoped to one tenant — what every production caller
    /// supplies. Tests that want the unscoped path ask for it explicitly.
    fn opts_for(org: &str) -> SearchOptions {
        SearchOptions {
            org_id: Some(org.to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn add_then_search_returns_match() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/rust-async",
            "Async Rust patterns",
            "tokio futures and pinning",
            "org_a",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        let results = idx.search("rust async", &opts_for("org_a")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].url.contains("rust-async"));
        assert_eq!(results[0].provider, "tantivy_local");
        assert_eq!(results[0].rank, 1);
    }

    #[tokio::test]
    async fn empty_index_returns_no_results() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.flush().await.unwrap();
        let results = idx.search("anything", &opts_for("org_a")).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn org_filter_isolates_tenants() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/a",
            "alpha cats",
            "tabby and calico",
            "org_a",
        ))
        .await
        .unwrap();
        idx.add_document(doc(
            "https://example.com/b",
            "beta cats",
            "siamese and persian",
            "org_b",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        let opts = SearchOptions::default();
        let results = idx
            .search_with_org("cats", Some("org_a"), &opts)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].url.ends_with("/a"));

        let results_b = idx
            .search_with_org("cats", Some("org_b"), &opts)
            .await
            .unwrap();
        assert_eq!(results_b.len(), 1);
        assert!(results_b[0].url.ends_with("/b"));
    }

    // ── tenant guard: write side ────────────────────────────────────────────

    #[tokio::test]
    async fn document_with_unknown_org_is_refused() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        for org in ["", "   "] {
            let err = idx
                .add_document(doc(
                    "https://example.com/orphan",
                    "orphan page",
                    "nobody owns this",
                    org,
                ))
                .await
                .expect_err("an org-less document must be refused, not stored under \"\"");
            assert_eq!(err.code, ErrorCode::BadRequest);
        }
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 0, "nothing should have been indexed");
    }

    // ── tenant guard: read side ─────────────────────────────────────────────

    #[tokio::test]
    async fn unscoped_search_cannot_see_another_orgs_documents() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/secret",
            "quarterly revenue",
            "confidential figures for org_a",
            "org_a",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        // No org on the options at all — the shape a caller that forgot to
        // thread the JWT org_id produces. It must not see org_a's document.
        let unscoped = idx
            .search("quarterly revenue", &SearchOptions::default())
            .await
            .unwrap();
        assert!(
            unscoped.is_empty(),
            "unscoped read leaked org_a documents: {unscoped:?}"
        );

        // An empty-string org is the same failure wearing a different hat.
        let empty_org = idx
            .search("quarterly revenue", &opts_for(""))
            .await
            .unwrap();
        assert!(empty_org.is_empty(), "empty org_id leaked: {empty_org:?}");

        // A different tenant asking by name sees nothing either.
        let other = idx
            .search("quarterly revenue", &opts_for("org_b"))
            .await
            .unwrap();
        assert!(other.is_empty(), "cross-tenant read leaked: {other:?}");

        // And the owner still gets its own document, so the guard above is
        // denying the right thing rather than breaking the index.
        let owner = idx
            .search("quarterly revenue", &opts_for("org_a"))
            .await
            .unwrap();
        assert_eq!(owner.len(), 1);
    }

    #[tokio::test]
    async fn search_all_orgs_is_the_explicit_operator_escape_hatch() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/a",
            "shared token alpha",
            "body",
            "org_a",
        ))
        .await
        .unwrap();
        idx.add_document(doc(
            "https://example.com/b",
            "shared token beta",
            "body",
            "org_b",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        let all = idx
            .search_all_orgs("shared token", &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(all.len(), 2, "operator read sees every tenant by design");
    }

    // ── upsert ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn reindex_same_url_yields_exactly_one_document() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/page",
            "old title",
            "old body",
            "org_a",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();
        idx.add_document(doc(
            "https://example.com/page",
            "new title",
            "new body",
            "org_a",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        // One document, not two competing copies of the same URL.
        assert_eq!(idx.doc_count(), 1);

        let results = idx.search("new", &opts_for("org_a")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title.as_deref(), Some("new title"));
        let old_results = idx.search("old", &opts_for("org_a")).await.unwrap();
        assert!(old_results.is_empty(), "old copy should be tombstoned");
    }

    #[tokio::test]
    async fn repeated_refetch_within_one_commit_still_yields_one_document() {
        // The crawl case: the same URL re-fetched several times before the
        // commit ticker fires. `delete_term` applies to documents added
        // earlier in the same uncommitted batch, so this must not accumulate.
        let idx = TantivyLocalIndex::in_memory().unwrap();
        for i in 0..5 {
            idx.add_document(doc(
                "https://example.com/hot",
                &format!("revision {i}"),
                "body text",
                "org_a",
            ))
            .await
            .unwrap();
        }
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 1);
    }

    // ── retention ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn retention_evicts_by_age() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc_aged(
            "https://example.com/ancient",
            "shared marker",
            "fetched a hundred days ago",
            "org_a",
            100,
        ))
        .await
        .unwrap();
        idx.add_document(doc_aged(
            "https://example.com/recent",
            "shared marker",
            "fetched yesterday",
            "org_a",
            1,
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 2);

        // Ceiling disabled — this pass isolates the age bound.
        let outcome = idx.enforce_retention_with(90, 0).await.unwrap();
        assert!(outcome.age_pass_ran);
        assert_eq!(outcome.docs_after, 1);

        let results = idx
            .search("shared marker", &opts_for("org_a"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].url.ends_with("/recent"));
    }

    #[tokio::test]
    async fn retention_age_pass_keeps_everything_inside_the_window() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        for age in [1, 30, 89] {
            idx.add_document(doc_aged(
                &format!("https://example.com/age-{age}"),
                "shared marker",
                "body",
                "org_a",
                age,
            ))
            .await
            .unwrap();
        }
        idx.flush().await.unwrap();

        let outcome = idx.enforce_retention_with(90, 0).await.unwrap();
        assert_eq!(outcome.docs_after, 3);
        assert_eq!(outcome.evicted_by_ceiling, 0);
    }

    #[tokio::test]
    async fn retention_evicts_oldest_first_to_reach_the_ceiling() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        // Ages 5,4,3,2,1 days — all well inside any age window, so only the
        // ceiling can evict here.
        for age in (1..=5).rev() {
            idx.add_document(doc_aged(
                &format!("https://example.com/age-{age}"),
                "shared marker",
                "body",
                "org_a",
                age,
            ))
            .await
            .unwrap();
        }
        idx.flush().await.unwrap();
        assert_eq!(idx.doc_count(), 5);

        // Age bound disabled — this pass isolates the ceiling.
        let outcome = idx.enforce_retention_with(0, 3).await.unwrap();
        assert!(!outcome.age_pass_ran);
        assert_eq!(outcome.evicted_by_ceiling, 2);
        assert_eq!(outcome.docs_after, 3);

        let results = idx
            .search("shared marker", &opts_for("org_a"))
            .await
            .unwrap();
        let surviving: Vec<&str> = results.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(surviving.len(), 3);
        // The two oldest went first; the three newest survived.
        for age in [5, 4] {
            let evicted = format!("https://example.com/age-{age}");
            assert!(
                !surviving.contains(&evicted.as_str()),
                "oldest document {evicted} should have been evicted, got {surviving:?}"
            );
        }
        for age in [3, 2, 1] {
            let kept = format!("https://example.com/age-{age}");
            assert!(
                surviving.contains(&kept.as_str()),
                "newer document {kept} should have survived, got {surviving:?}"
            );
        }
    }

    #[tokio::test]
    async fn retention_is_a_no_op_when_both_bounds_are_disabled() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc_aged(
            "https://example.com/very-old",
            "shared marker",
            "body",
            "org_a",
            5_000,
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        let outcome = idx.enforce_retention_with(0, 0).await.unwrap();
        assert!(!outcome.age_pass_ran);
        assert_eq!(outcome.evicted_by_ceiling, 0);
        assert_eq!(outcome.docs_after, 1);
    }

    #[test]
    fn retention_defaults_are_bounded_not_disabled() {
        // A default of 0 would silently disable the pass it belongs to, which
        // is the exact failure this retention work exists to prevent. The env
        // knobs themselves are read at call time and deliberately not
        // exercised here — they are process-global and these tests run in
        // parallel; `enforce_retention_with` is the seam that takes bounds
        // explicitly.
        assert!(DEFAULT_RETENTION_DAYS > 0);
        assert!(DEFAULT_MAX_DOCS > 0);
    }

    // ── commit / durability ─────────────────────────────────────────────────

    #[tokio::test]
    async fn flush_commits_pending_writes_and_clears_the_counter() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        assert_eq!(idx.pending_writes(), 0);

        for i in 0..3 {
            idx.add_document(doc(
                &format!("https://example.com/p{i}"),
                "shared marker",
                "body",
                "org_a",
            ))
            .await
            .unwrap();
        }
        assert_eq!(
            idx.pending_writes(),
            3,
            "uncommitted writes must be visible to the commit ticker"
        );
        // Uncommitted writes are not yet searchable — which is exactly why a
        // restart without a flush loses them.
        let before = idx
            .search("shared marker", &opts_for("org_a"))
            .await
            .unwrap();
        assert!(before.is_empty());

        idx.flush().await.unwrap();
        assert_eq!(idx.pending_writes(), 0);
        let after = idx
            .search("shared marker", &opts_for("org_a"))
            .await
            .unwrap();
        assert_eq!(after.len(), 3);
    }

    #[tokio::test]
    async fn refused_document_does_not_count_as_pending() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        let _ = idx
            .add_document(doc("https://example.com/x", "t", "b", ""))
            .await;
        assert_eq!(idx.pending_writes(), 0);
    }

    // ── ranking ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn title_boost_outranks_body_only_match() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.add_document(doc(
            "https://example.com/title-match",
            "tokio runtime guide",
            "unrelated content",
            "org_a",
        ))
        .await
        .unwrap();
        idx.add_document(doc(
            "https://example.com/body-only",
            "rust networking",
            "tokio runtime is mentioned only here",
            "org_a",
        ))
        .await
        .unwrap();
        idx.flush().await.unwrap();

        let results = idx
            .search("tokio runtime", &opts_for("org_a"))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].url.contains("title-match"));
    }

    #[tokio::test]
    async fn limit_caps_result_count() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        for i in 0..20 {
            idx.add_document(doc(
                &format!("https://example.com/page-{i}"),
                "shared title token",
                "shared body token",
                "org_a",
            ))
            .await
            .unwrap();
        }
        idx.flush().await.unwrap();

        let opts = SearchOptions {
            limit: 5,
            ..opts_for("org_a")
        };
        let results = idx.search("shared", &opts).await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn provider_name_is_tantivy_local() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        assert_eq!(idx.name(), "tantivy_local");
    }
}
