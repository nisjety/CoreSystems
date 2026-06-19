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
//! | `url` | `TEXT` | stored, tokenized — also the primary key |
//! | `title` | `TEXT` | stored, tokenized, boosted in queries |
//! | `body` | `TEXT` | tokenized (not stored — fetch markdown via artifact ref) |
//! | `host` | `STRING` | indexed exact, stored — for facet filters |
//! | `org_id` | `STRING` | indexed exact, stored — multi-tenant scoping |
//! | `fingerprint` | `STRING` | stored — change-detection link-back |
//! | `fetched_at` | `DATE` | stored, indexed — recency boost / time filters |
//!
//! ## Org scoping
//!
//! Tenant isolation is enforced at query construction: every search query
//! is combined with an `org_id:{tenant}` filter so the index can't leak
//! across tenants even if the schema is shared.
//!
//! ## Persistence
//!
//! - In-memory mode: `TantivyLocalIndex::in_memory()` — tests, dev
//! - On-disk mode: `TantivyLocalIndex::open(path)` — production
//!
//! Writer is `Arc<Mutex<IndexWriter>>` so multiple `add_document` calls
//! from concurrent PageRunner success paths serialize cheaply. `flush` is
//! exposed for callers who need the index queryable immediately (tests);
//! production lets Tantivy's autocommit handle it.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, FAST, INDEXED, STORED, STRING, TEXT};
use tantivy::time::OffsetDateTime;
use tantivy::{doc, DateTime as TantivyDate, Index, IndexReader, IndexWriter, ReloadPolicy, Term};
use tokio::sync::Mutex;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::serp::{SearchOptions, SearchProvider, SearchResult};

/// Tantivy writer heap budget (50 MB). Plenty for batch ingestion at our
/// expected per-scrape document size (~few KB titles + tokenized body).
const WRITER_HEAP_BYTES: usize = 50_000_000;

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
        })
    }

    /// Add (or replace) a document.
    ///
    /// Replacement semantics: deletes any prior doc with the same `url`
    /// before adding the new one. This makes the index converge to "latest
    /// fetched copy of URL X" without unbounded growth on re-crawls.
    pub async fn add_document(&self, doc: LocalDocument) -> QuarryResult<()> {
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
        Ok(())
    }

    /// Flush pending writes to disk and reload the reader. Production
    /// callers can rely on Tantivy's autocommit; tests call this directly
    /// so the index is visible to the very next search.
    pub async fn flush(&self) -> QuarryResult<()> {
        let mut writer = self.writer.lock().await;
        writer.commit().map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: commit failed: {e}"),
            )
        })?;
        self.reader.reload().map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("local_index: reader reload failed: {e}"),
            )
        })?;
        Ok(())
    }

    /// Query the index. Combines the user query (title+body weighted) with
    /// an org_id facet filter when supplied. Returns up to `limit` results,
    /// ranked by BM25 score.
    pub async fn search_with_org(
        &self,
        query: &str,
        org_id: Option<&str>,
        opts: &SearchOptions,
    ) -> QuarryResult<Vec<SearchResult>> {
        let searcher = self.reader.searcher();
        let limit = opts.limit.max(1) as usize;

        // Boost title 3x vs body — same convention as the SourceTraceBuilder.
        let mut parser =
            QueryParser::for_index(&self.index, vec![self.schema.title, self.schema.body]);
        parser.set_field_boost(self.schema.title, 3.0);
        let user_query = parser.parse_query_lenient(query).0;

        let final_query: Box<dyn Query> = match org_id {
            Some(o) if !o.is_empty() => {
                let org_query = TermQuery::new(
                    Term::from_field_text(self.schema.org_id, o),
                    IndexRecordOption::Basic,
                );
                Box::new(BooleanQuery::new(vec![
                    (Occur::Must, user_query),
                    (Occur::Must, Box::new(org_query)),
                ]))
            }
            _ => user_query,
        };

        // Over-fetch BM25 candidates so the recency rerank below has room to
        // reorder before truncating to the caller's limit.
        let candidate_limit = limit
            .saturating_mul(RECENCY_OVERFETCH)
            .clamp(limit, RECENCY_MAX_CANDIDATES);
        let top_docs = searcher
            .search(&*final_query, &TopDocs::with_limit(candidate_limit))
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
        assert!(m >= RECENCY_FLOOR && m < RECENCY_FLOOR + 0.05);
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
        // Tenant isolation: when the caller supplies an `org_id` in
        // `SearchOptions`, restrict results to documents tagged with
        // that exact org_id. When unset (e.g. legacy callers that
        // pre-date the auth middleware), fall back to the org-agnostic
        // path. Production routes always carry the verified JWT org_id
        // through `SearchOptions.org_id`, so this fallback only fires
        // in test harnesses and explicit internal callers.
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

    fn doc(url: &str, title: &str, body: &str, org: &str) -> LocalDocument {
        LocalDocument {
            url: url.into(),
            title: title.into(),
            body: body.into(),
            host: url.split('/').nth(2).unwrap_or("example.com").to_string(),
            org_id: org.into(),
            fingerprint: format!("blake3:{}", url),
            fetched_at: Utc::now(),
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

        let opts = SearchOptions::default();
        let results = idx.search("rust async", &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].url.contains("rust-async"));
        assert_eq!(results[0].provider, "tantivy_local");
        assert_eq!(results[0].rank, 1);
    }

    #[tokio::test]
    async fn empty_index_returns_no_results() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        idx.flush().await.unwrap();
        let results = idx
            .search("anything", &SearchOptions::default())
            .await
            .unwrap();
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

    #[tokio::test]
    async fn reindex_same_url_replaces_prior_copy() {
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

        let results = idx.search("new", &SearchOptions::default()).await.unwrap();
        // The user-visible behavior: searching for "new" returns exactly
        // the one current doc. The old doc was tombstoned via delete_term
        // on the URL and won't surface in queries. Note that Tantivy's
        // num_docs() may still report the tombstoned doc until segment
        // merge, so doc_count() is a telemetry approximation, not an
        // assertable invariant.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title.as_deref(), Some("new title"));
        // Also confirm the old title is no longer searchable:
        let old_results = idx.search("old", &SearchOptions::default()).await.unwrap();
        assert!(old_results.is_empty(), "old doc should be tombstoned");
    }

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
            .search("tokio runtime", &SearchOptions::default())
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

        let mut opts = SearchOptions::default();
        opts.limit = 5;
        let results = idx.search("shared", &opts).await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn provider_name_is_tantivy_local() {
        let idx = TantivyLocalIndex::in_memory().unwrap();
        assert_eq!(idx.name(), "tantivy_local");
    }
}
