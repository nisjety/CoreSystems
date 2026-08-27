use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::pipeline::types::ScoredCandidate;

#[async_trait]
pub trait SparseSearchBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>>;
}

pub type DynSparseSearchBackend = Arc<dyn SparseSearchBackend>;

/// How many times `top_k` to request from Quickwit before de-duplicating.
///
/// Duplicates arise from Quickwit's asynchronous deletes (see the de-dup block
/// in `QuickwitSparseBackend::search`), and a duplicate consumes a slot that a
/// distinct chunk should have had. 4x covers the ~4.5x duplication observed on
/// a corpus re-driven several times while still bounding the response size; the
/// de-dup loop stops as soon as it has `top_k` distinct chunks, so the extra
/// hits cost nothing on a clean index.
const DEDUP_OVERFETCH: usize = 4;

/// Absolute ceiling on `max_hits`, so a large `top_k` cannot ask Quickwit for
/// an unbounded page.
const MAX_HITS: usize = 500;

pub struct PostgresSparseBackend {
    pool: PgPool,
}

impl PostgresSparseBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SparseSearchBackend for PostgresSparseBackend {
    fn name(&self) -> &'static str {
        "postgres"
    }

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        bm25_search(&self.pool, query, org_id, top_k).await
    }
}

pub struct QuickwitSparseBackend {
    client: Client,
    base_url: String,
    index_id: String,
}

impl QuickwitSparseBackend {
    pub fn new(base_url: impl Into<String>, index_id: impl Into<String>, timeout_ms: u64) -> Self {
        let timeout = Duration::from_millis(timeout_ms.max(100));
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            index_id: index_id.into(),
        }
    }
}

#[async_trait]
impl SparseSearchBackend for QuickwitSparseBackend {
    fn name(&self) -> &'static str {
        "quickwit"
    }

    #[tracing::instrument(
        name = "quickwit.sparse_search",
        skip(self, query),
        fields(org_id = org_id, top_k = top_k)
    )]
    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if query.trim().is_empty() || top_k == 0 {
            return Ok(Vec::new());
        }

        let quickwit_query = build_quickwit_query(org_id, query);
        let url = format!("{}/api/v1/{}/search", self.base_url, self.index_id);
        // Over-fetch, because the hits are de-duplicated below and duplicates
        // would otherwise silently shrink the arm's contribution. See
        // `DEDUP_OVERFETCH`.
        let requested = top_k.saturating_mul(DEDUP_OVERFETCH).min(MAX_HITS);
        let body = serde_json::json!({
            "query": quickwit_query,
            "max_hits": requested
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let parsed: QuickwitSearchResponse = response.json().await?;

        Ok(dedup_hits(parsed.hits, requested, top_k))
    }
}

pub struct FallbackSparseBackend {
    primary: DynSparseSearchBackend,
    fallback: DynSparseSearchBackend,
}

impl FallbackSparseBackend {
    pub fn new(primary: DynSparseSearchBackend, fallback: DynSparseSearchBackend) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl SparseSearchBackend for FallbackSparseBackend {
    fn name(&self) -> &'static str {
        "quickwit-with-postgres-fallback"
    }

    async fn search(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        match self.primary.search(query, org_id, top_k).await {
            Ok(candidates) => Ok(candidates),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    primary = self.primary.name(),
                    fallback = self.fallback.name(),
                    "sparse search backend failed; falling back"
                );
                self.fallback.search(query, org_id, top_k).await
            }
        }
    }
}

#[tracing::instrument(
    name = "postgres.bm25_search",
    skip(pool, query),
    fields(
        otel.kind = "client",
        db.system = "postgresql",
        org_id = org_id,
        top_k = top_k,
    ),
)]
pub async fn bm25_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    top_k: usize,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    // §16.3.3 — uses the precomputed `content_tsv` generated column (GIN
    // indexed) instead of re-tokenizing per query. The query planner picks
    // the GIN index over the previous expression index automatically.
    //
    // Phase 1 RLS: a retrieval request serves exactly one org (taken from the
    // verified caller claims), so this reads through an org-scoped transaction.
    // The SQL still binds `org_id` itself — the database policy is a backstop
    // against that filter being dropped or mis-edited later, not a replacement.
    // The terms are OR-joined and matched with `websearch_to_tsquery`, NOT
    // `plainto_tsquery`. `plainto_tsquery` ANDs every word (`'a' & 'b' & …`), so
    // a natural-language question required all of its words inside one
    // 512-char chunk — measured on the live corpus, "how does retrieval combine
    // dense sparse and graph results" matched 0 rows that way and 1,161 as a
    // disjunction. This arm was returning nothing for realistic queries and
    // reporting no error, exactly like the Quickwit path (see
    // `build_quickwit_query`). Union-then-rank is what a BM25-style arm is for;
    // `ts_rank_cd` below does the ranking, and rare terms dominate it, so common
    // words dilute the score rather than filtering everything out.
    //
    // `websearch_to_tsquery` rather than a hand-built `to_tsquery` string
    // because it is total on user input: it never raises a syntax error, so the
    // sanitized terms cannot combine into something that fails the query.
    // Verified: `a or or or -- \\ ) | & ! or b` parses to `'a' | 'or' | 'b'`.
    // Anchored: when the query names an identifier/path, search on that rather
    // than OR-ing the surrounding prose in alongside it. `websearch_to_tsquery`
    // has no "required plus optional" form, so prose terms could only add rows
    // here, never reorder them — the flood with none of the upside.
    let fts_query = crate::search::textquery::fts_anchor_disjunction(query);
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_as::<_, BM25Row>(
        r#"
        SELECT
            knowledge_id,
            document_id,
            text,
            ts_rank_cd(content_tsv, websearch_to_tsquery('simple', $1)) AS rank_score,
            chunk_index,
            metadata
        FROM knowledge_units
        WHERE org_id = $2
          AND content_tsv @@ websearch_to_tsquery('simple', $1)
          -- Phase 4 read-your-writes: the sparse (FTS) arm needs no vectors, so
          -- surface just-chunked content immediately (status 'pending') instead
          -- of waiting for the async embed. Exclude only 'failed'. The ownership
          -- post-filter still gates by viewer; the dense arm joins once embedded
          -- and RRF fusion dedups by knowledge_id (no double-count).
          AND embedding_status <> 'failed'
        ORDER BY rank_score DESC
        LIMIT $3
        "#,
    )
    .bind(&fts_query)
    .bind(org_id)
    .bind(top_k as i64)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(rows
        .into_iter()
        .map(|r| ScoredCandidate {
            knowledge_id: r.knowledge_id,
            document_id: r.document_id,
            text: r.text,
            dense_score: 0.0,
            sparse_score: r.rank_score,
            rerank_score: 0.0,
            final_score: r.rank_score,
            chunk_index: r.chunk_index,
            metadata: std::collections::HashMap::new(),
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct BM25Row {
    knowledge_id: String,
    document_id: String,
    text: String,
    rank_score: f32,
    chunk_index: i32,
    #[allow(dead_code)]
    metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct QuickwitSearchResponse {
    #[serde(default)]
    hits: Vec<QuickwitHit>,
}

#[derive(Debug, Deserialize)]
struct QuickwitHit {
    #[serde(default)]
    score: Option<f32>,
    #[serde(default)]
    json: Option<Value>,
    #[serde(default)]
    doc: Option<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl QuickwitHit {
    fn into_candidate(self, top_k: usize, idx: usize) -> Option<ScoredCandidate> {
        let doc = if let Some(json) = self.json {
            json
        } else if let Some(doc) = self.doc {
            doc
        } else if !self.extra.is_empty() {
            Value::Object(self.extra)
        } else {
            return None;
        };

        let knowledge_id = doc.get("knowledge_id")?.as_str()?.to_string();
        let document_id = doc.get("document_id")?.as_str()?.to_string();
        let text = doc
            .get("body")
            .or_else(|| doc.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let chunk_index = doc
            .get("chunk_index")
            .and_then(Value::as_i64)
            .unwrap_or_default() as i32;
        let sparse_score = self
            .score
            .unwrap_or_else(|| (top_k.saturating_sub(idx)) as f32);

        Some(ScoredCandidate {
            knowledge_id,
            document_id,
            text,
            dense_score: 0.0,
            sparse_score,
            rerank_score: 0.0,
            final_score: sparse_score,
            chunk_index,
            metadata: std::collections::HashMap::new(),
        })
    }
}

/// Collapse Quickwit hits to at most `top_k` distinct chunks, keeping the
/// best-ranked occurrence of each `knowledge_id`.
///
/// Quickwit is append-only and its `delete_by_query` is ASYNCHRONOUS — the
/// delete becomes a task applied at merge time, not before the next write. The
/// adapter's re-index path (`documents.indexed`) deletes and then immediately
/// re-indexes, so every content update or re-crawl leaves the previous copies
/// in place alongside the new ones until a merge catches up. Measured on a
/// 1,164-chunk corpus that had been re-driven a few times: 5,215 hits for 1,164
/// distinct chunks.
///
/// Fusion already keys on `knowledge_id`, so duplicates could never corrupt the
/// final ranking — but they consume this arm's `top_k` slots BEFORE fusion sees
/// them, which quietly collapses sparse recall (30 hits that are really 8
/// chunks). De-duplicating makes the arm correct for whatever state the index is
/// in, rather than assuming it is clean — which, for an eventually-consistent
/// store, it periodically is not.
fn dedup_hits(hits: Vec<QuickwitHit>, requested: usize, top_k: usize) -> Vec<ScoredCandidate> {
    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(top_k);
    let mut out: Vec<ScoredCandidate> = Vec::with_capacity(top_k);
    for (idx, hit) in hits.into_iter().enumerate() {
        let Some(candidate) = hit.into_candidate(requested, idx) else {
            continue;
        };
        if !seen.insert(candidate.knowledge_id.clone()) {
            continue;
        }
        out.push(candidate);
        if out.len() >= top_k {
            break;
        }
    }
    out
}

/// Build the Quickwit query for one lexical search.
///
/// The user's terms become a DISJUNCTION — `("a" OR "b" OR "c")` — inside the
/// tenant filter. Two reasons, both learned the hard way:
///
/// 1. Quickwit's default operator is AND, and this function used to join terms
///    with a bare space. So a natural-language question required every one of
///    its words to co-occur inside a single 512-char chunk, which essentially
///    never happens: measured against a live 1,164-chunk corpus, "how does
///    retrieval combine dense sparse and graph results" returned 0 hits joined
///    by spaces and 5,460 joined by OR. The lexical arm was silently returning
///    nothing for every realistic query while looking perfectly healthy — the
///    fusion step just had no sparse candidates to weigh. Union-then-rank is
///    also what BM25 *is*; requiring all terms is boolean retrieval wearing a
///    BM25 label, and Quickwit already ranks the matched set by BM25, so common
///    words are down-weighted by IDF rather than needing a stopword list.
///
/// 2. Every term is quoted, which is what makes the disjunction safe. Terms
///    come from user input, and `AND` / `OR` / `NOT` survive sanitization as
///    ordinary words — unquoted, they are parsed as operators. A query ending
///    in "or" used to produce a dangling operator and a hard 400 from Quickwit;
///    quoted, it is just a term (verified live: a query containing bare `OR AND
///    OR` fails to parse, the quoted form returns hits).
fn build_quickwit_query(org_id: &str, query: &str) -> String {
    let terms = sanitize_terms(query);
    let org_filter = format!(
        "org_id:{} AND entity_type:{}",
        quote_term(org_id),
        quote_term("knowledge_unit")
    );
    if terms.is_empty() {
        return org_filter;
    }

    let quoted = |list: &[String]| {
        list.iter()
            .map(|term| quote_term(term))
            .collect::<Vec<_>>()
            .join(" OR ")
    };

    // Anchor on the distinctive terms when the query has any. A rare term OR'd
    // with common words is swamped by them — measured 1,517 hits with the target
    // in none of the top 5, against 14 hits and 5/5 once the term is required.
    // `^boost` was tried first and Quickwit ranks identically, so requiring is
    // the only lever that moves anything. See `textquery::distinctive_terms` for
    // why trading the lexical arm's recall for precision is right in a fused
    // system.
    //
    // Ordinary terms stay as an OPTIONAL clause here (unlike the Postgres arm,
    // which cannot express that) so they still shape BM25 order within the
    // anchored set.
    let distinctive = crate::search::textquery::distinctive_terms(query);
    if distinctive.is_empty() {
        return format!("{org_filter} AND ({})", quoted(&terms));
    }

    let ordinary: Vec<String> = terms
        .iter()
        .filter(|term| !distinctive.contains(term))
        .cloned()
        .collect();
    let anchor = format!("+({})", quoted(&distinctive));
    if ordinary.is_empty() {
        format!("{org_filter} AND {anchor}")
    } else {
        format!("{org_filter} AND {anchor} AND ({})", quoted(&ordinary))
    }
}

/// Reduce a user query to bare terms. Delegates to
/// [`crate::search::textquery::sanitize_terms`] — the same sanitization now
/// backs the Quickwit disjunction here, both Postgres graph tiers and the
/// contradictions claim search, so there is one definition rather than four
/// copies that can drift apart.
fn sanitize_terms(query: &str) -> Vec<String> {
    crate::search::textquery::sanitize_terms(query)
}

fn quote_term(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StaticBackend {
        name: &'static str,
        error: Option<&'static str>,
        candidates: Vec<ScoredCandidate>,
        calls: AtomicUsize,
    }

    impl StaticBackend {
        fn ok(name: &'static str, candidates: Vec<ScoredCandidate>) -> Self {
            Self {
                name,
                error: None,
                candidates,
                calls: AtomicUsize::new(0),
            }
        }

        fn fail(name: &'static str, error: &'static str) -> Self {
            Self {
                name,
                error: Some(error),
                candidates: Vec::new(),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SparseSearchBackend for StaticBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn search(
            &self,
            _query: &str,
            _org_id: &str,
            _top_k: usize,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.error {
                anyhow::bail!(error);
            }
            Ok(self.candidates.clone())
        }
    }

    fn candidate(id: &str) -> ScoredCandidate {
        ScoredCandidate {
            knowledge_id: id.to_string(),
            document_id: format!("doc-{id}"),
            text: format!("text-{id}"),
            dense_score: 0.0,
            sparse_score: 1.0,
            rerank_score: 0.0,
            final_score: 1.0,
            chunk_index: 0,
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn quickwit_query_scopes_to_org_and_knowledge_units() {
        let query = build_quickwit_query("org-1", "alpha beta site:ignored ../bad");

        assert!(query.contains("org_id:\"org-1\""));
        assert!(query.contains("entity_type:\"knowledge_unit\""));
        assert!(query.contains("\"alpha\" OR \"beta\""));
        // The caller's field selector must not survive as a selector.
        assert!(!query.contains("site:ignored"));
        // `/` USED to be stripped here, and this test asserted that. It was
        // wrong: a path-shaped term matched nothing once its separators were
        // removed (measured 153 Quickwit hits -> 0 for `src/api/mod.rs`), so the
        // slash now survives. What makes that safe is quoting, not stripping —
        // assert the quoting instead of the mangling.
        assert!(
            query.contains("\"../bad\""),
            "a path-shaped term must survive, quoted: {query}"
        );
    }

    /// The path fix, at this boundary: slashes reach Quickwit inside a quoted
    /// phrase, so they are matched as text and can never act as query syntax.
    #[test]
    fn file_paths_reach_quickwit_quoted() {
        let query = build_quickwit_query("org-1", "where is src/api/mod.rs used");

        assert!(
            query.contains("\"src/api/mod.rs\""),
            "the path must be present and quoted: {query}"
        );
        // Quoted means the `/` is inside the phrase, never adjacent to the
        // `field:value` syntax Quickwit would otherwise try to parse.
        assert!(
            !query.contains("/api/mod.rs\" OR \"src"),
            "the path must not be split across terms: {query}"
        );
    }

    // Regression: Quickwit's default operator is AND, so joining terms with a
    // bare space made a natural-language question require every word inside one
    // 512-char chunk. Measured live: 0 hits space-joined vs 5,460 OR-joined on
    // the same query and corpus. The arm returned nothing for realistic queries
    // while reporting no error at all.
    #[test]
    fn multi_term_queries_are_a_disjunction_not_a_conjunction() {
        let query = build_quickwit_query(
            "org-1",
            "how does retrieval combine dense sparse and graph results",
        );

        let terms = query
            .split_once(" AND (")
            .expect("term group present")
            .1
            .trim_end_matches(')');
        assert!(
            !terms.contains(" AND "),
            "terms must not be ANDed together: {terms}"
        );
        assert_eq!(
            terms.matches(" OR ").count(),
            8,
            "9 terms should yield 8 OR joins: {terms}"
        );
    }

    // Terms come from user input, and AND/OR/NOT survive sanitization as words.
    // Unquoted they parse as operators — a dangling one is a hard 400 from
    // Quickwit. Quoting every term makes them ordinary terms instead.
    #[test]
    fn operator_keywords_in_user_input_cannot_become_operators() {
        let query = build_quickwit_query("org-1", "cats OR AND NOT dogs");

        let terms = query
            .split_once(" AND (")
            .expect("term group present")
            .1
            .trim_end_matches(')');
        // Every user word is quoted; the only unquoted OR is the join itself.
        for word in ["cats", "OR", "AND", "NOT", "dogs"] {
            assert!(
                terms.contains(&format!("\"{word}\"")),
                "{word} should appear quoted in {terms}"
            );
        }
        // 5 terms -> 4 joins. If a bare keyword had leaked through as an
        // operator the count would differ and Quickwit would fail to parse.
        assert_eq!(terms.matches(" OR ").count(), 4, "{terms}");
    }

    #[test]
    fn a_query_of_only_punctuation_falls_back_to_the_tenant_filter() {
        let query = build_quickwit_query("org-1", "??? !!! :::");

        assert!(query.contains("org_id:\"org-1\""));
        assert!(
            !query.contains(" AND ("),
            "no term group should be emitted: {query}"
        );
    }

    #[test]
    fn the_term_count_is_capped() {
        let long = (0..100)
            .map(|i| format!("term{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let query = build_quickwit_query("org-1", &long);
        // `term0`..`term99` are digit+letter, so every one is a distinctive
        // anchor and they land inside `+( ... )` rather than the optional
        // clause. The org filter contains no ` OR `, so counting across the
        // whole query is exact and survives either shape.
        assert_eq!(query.matches(" OR ").count(), 31, "32 terms -> 31 joins");
    }

    #[tokio::test]
    async fn fallback_backend_uses_postgres_when_quickwit_fails() {
        let primary = Arc::new(StaticBackend::fail("quickwit", "connection refused"));
        let fallback = Arc::new(StaticBackend::ok("postgres", vec![candidate("k1")]));
        let backend = FallbackSparseBackend::new(primary.clone(), fallback.clone());

        let results = backend.search("alpha", "org-1", 10).await.unwrap();

        assert_eq!(backend.name(), "quickwit-with-postgres-fallback");
        assert_eq!(primary.calls(), 1);
        assert_eq!(fallback.calls(), 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].knowledge_id, "k1");
    }

    #[tokio::test]
    async fn fallback_backend_does_not_call_postgres_when_quickwit_succeeds() {
        let primary = Arc::new(StaticBackend::ok("quickwit", vec![candidate("k2")]));
        let fallback = Arc::new(StaticBackend::ok("postgres", vec![candidate("k3")]));
        let backend = FallbackSparseBackend::new(primary.clone(), fallback.clone());

        let results = backend.search("alpha", "org-1", 10).await.unwrap();

        assert_eq!(primary.calls(), 1);
        assert_eq!(fallback.calls(), 0);
        assert_eq!(results[0].knowledge_id, "k2");
    }

    fn quickwit_hit(knowledge_id: &str, score: f32) -> QuickwitHit {
        serde_json::from_value(serde_json::json!({
            "score": score,
            "knowledge_id": knowledge_id,
            "document_id": "doc-1",
            "body": "text",
            "chunk_index": 0,
        }))
        .expect("hit fixture parses")
    }

    // Regression: an index carrying duplicates must still yield `top_k` DISTINCT
    // chunks. Before de-duplication this arm returned `top_k` *rows*, which on a
    // re-indexed corpus meant a handful of chunks repeated — sparse recall
    // collapsed silently, because every row looked like a legitimate hit.
    #[test]
    fn dedup_hits_returns_distinct_chunks_from_a_duplicated_index() {
        // Four copies of every chunk, interleaved the way Quickwit returns them
        // when several splits each hold a generation of the same document.
        let mut hits = Vec::new();
        for copy in 0..4 {
            for chunk in 0..10 {
                hits.push(quickwit_hit(
                    &format!("k{chunk}"),
                    100.0 - (chunk as f32) - (copy as f32) * 0.01,
                ));
            }
        }
        assert_eq!(hits.len(), 40);

        let got = dedup_hits(hits, 40, 10);

        assert_eq!(got.len(), 10, "should fill top_k with distinct chunks");
        let ids: Vec<&str> = got.iter().map(|c| c.knowledge_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9"],
            "first occurrence of each chunk wins, and Quickwit's order is preserved"
        );
    }

    #[test]
    fn dedup_hits_keeps_the_highest_scoring_copy() {
        // Quickwit returns hits in descending score order, so the first copy of
        // a chunk is its best-scoring copy — the survivor must carry that score,
        // not a later duplicate's.
        let got = dedup_hits(
            vec![
                quickwit_hit("k1", 9.5),
                quickwit_hit("k1", 2.0),
                quickwit_hit("k2", 1.0),
            ],
            3,
            10,
        );

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].knowledge_id, "k1");
        assert!(
            (got[0].sparse_score - 9.5).abs() < f32::EPSILON,
            "kept copy should score 9.5, got {}",
            got[0].sparse_score
        );
        assert_eq!(got[1].knowledge_id, "k2");
    }

    #[test]
    fn dedup_hits_is_a_no_op_on_a_clean_index() {
        let got = dedup_hits(
            vec![quickwit_hit("k1", 3.0), quickwit_hit("k2", 2.0)],
            8,
            10,
        );

        assert_eq!(got.len(), 2, "fewer hits than top_k must pass through");
        assert_eq!(got[0].knowledge_id, "k1");
        assert_eq!(got[1].knowledge_id, "k2");
    }

    #[test]
    fn quickwit_hit_parses_flat_hit_shape() {
        let hit: QuickwitHit = serde_json::from_value(serde_json::json!({
            "score": 7.5,
            "knowledge_id": "knowledge-1",
            "document_id": "document-1",
            "body": "body text",
            "chunk_index": 4
        }))
        .unwrap();

        let candidate = hit.into_candidate(10, 0).unwrap();

        assert_eq!(candidate.knowledge_id, "knowledge-1");
        assert_eq!(candidate.document_id, "document-1");
        assert_eq!(candidate.text, "body text");
        assert_eq!(candidate.chunk_index, 4);
        assert_eq!(candidate.sparse_score, 7.5);
    }

    #[test]
    fn quickwit_hit_parses_nested_json_hit_shape() {
        let hit: QuickwitHit = serde_json::from_value(serde_json::json!({
            "json": {
                "knowledge_id": "knowledge-2",
                "document_id": "document-2",
                "text": "nested text"
            }
        }))
        .unwrap();

        let candidate = hit.into_candidate(5, 1).unwrap();

        assert_eq!(candidate.knowledge_id, "knowledge-2");
        assert_eq!(candidate.document_id, "document-2");
        assert_eq!(candidate.text, "nested text");
        assert_eq!(candidate.sparse_score, 4.0);
    }
}
