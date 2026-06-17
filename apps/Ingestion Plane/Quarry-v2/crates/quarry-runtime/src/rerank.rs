//! Semantic reranking + query-relevant highlights for search results.
//!
//! Exa-inspired precision layer: after the router/hybrid provider returns a
//! merged result set (good *recall*), an LLM reorders the top-N by relevance to
//! the query and extracts a short query-relevant highlight for each. Wrapped as
//! a [`crate::serp::SearchProvider`] ([`RerankingSearchProvider`]) so it slots
//! into `AppState.search` transparently and its output is cached by the edge.
//!
//! Degrade-safe: any Model Plane failure or unparseable reply returns the
//! original ordering unchanged (no scores, no highlights). The long tail beyond
//! `top_n` is never reordered — only the head the user is most likely to read.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use quarry_core::error::QuarryResult;

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};
use crate::serp::{SearchOptions, SearchProvider, SearchResult};

/// Reranks the top-N of a result set by relevance to the query, attaching a
/// `score` and `highlights` to each. Implementations MUST be degrade-safe:
/// return the input unchanged on any failure rather than erroring.
#[async_trait]
pub trait SearchReranker: Send + Sync {
    async fn rerank(
        &self,
        query: &str,
        results: Vec<SearchResult>,
        top_n: usize,
    ) -> Vec<SearchResult>;
}

/// Production reranker backed by the Model Plane. Falls back to the original
/// ordering on any failure.
pub struct ModelPlaneSearchReranker {
    client: Arc<ModelPlaneClient>,
    model: Option<String>,
}

impl ModelPlaneSearchReranker {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

#[derive(Debug, Deserialize)]
struct RerankEntry {
    index: usize,
    #[serde(default)]
    score: f32,
    #[serde(default)]
    highlight: Option<String>,
}

#[async_trait]
impl SearchReranker for ModelPlaneSearchReranker {
    async fn rerank(
        &self,
        query: &str,
        results: Vec<SearchResult>,
        top_n: usize,
    ) -> Vec<SearchResult> {
        let n = top_n.min(results.len());
        if n == 0 || query.trim().is_empty() {
            return results;
        }
        let prompt = build_rerank_prompt(query, &results[..n]);
        let req = ModelPlaneInvokeRequest {
            content: prompt,
            model: self.model.clone(),
            session_key: None,
            thread_id: None,
        };
        let body = match self.client.invoke(&req).await {
            Ok(resp) => resp.content,
            Err(e) => {
                tracing::warn!(error = %e, "rerank: model plane failed; keeping original order");
                return results;
            }
        };
        let stripped = strip_code_fences(&body);
        match serde_json::from_str::<Vec<RerankEntry>>(&stripped) {
            Ok(entries) => apply_rerank(results, entries, n),
            Err(e) => {
                tracing::warn!(error = %e, "rerank: JSON parse failed; keeping original order");
                results
            }
        }
    }
}

/// Build the rerank prompt from the head results. Each is labelled by its index
/// so the model returns a compact `[{index,score,highlight}]` array.
fn build_rerank_prompt(query: &str, head: &[SearchResult]) -> String {
    let mut sb = String::with_capacity(512 + head.len() * 256);
    sb.push_str(
        "You are a search-result reranker. Score how well each result answers the query.\n\n",
    );
    sb.push_str("Query: ");
    sb.push_str(query.trim());
    sb.push_str("\n\nResults:\n");
    for (i, r) in head.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(untitled)");
        let snippet = r.snippet.as_deref().unwrap_or("");
        sb.push_str(&format!("[{i}] {title} — {}\n{snippet}\n", r.url));
    }
    sb.push_str(
        "\nFor each result return its `index`, a `score` from 0.0 (irrelevant) to 1.0 \
(directly answers the query), and a `highlight`: the single most query-relevant \
sentence or phrase copied verbatim from that result's title/snippet (empty string \
if none). Return ONLY a JSON array, no prose.\n",
    );
    sb.push_str("Example: [{\"index\":0,\"score\":0.92,\"highlight\":\"...\"}]");
    sb
}

/// Pure reorder: apply the model's scored `entries` to the head `[..n]` of
/// `results`, sorting that head by descending score and attaching score +
/// highlight. Head results the model omitted keep their original relative order
/// and trail the scored ones (no score). The tail `[n..]` is untouched. Ranks
/// are renumbered 1-based. Extracted for unit testing without the network.
fn apply_rerank(
    mut results: Vec<SearchResult>,
    entries: Vec<RerankEntry>,
    n: usize,
) -> Vec<SearchResult> {
    let n = n.min(results.len());
    // Split off the tail; we only reorder the head.
    let tail = results.split_off(n);
    let head = results; // now exactly the first n

    // Map valid, unique indices → (score, highlight), first-occurrence wins.
    let mut scored: Vec<(usize, f32, Option<String>)> = Vec::with_capacity(head.len());
    let mut seen = vec![false; head.len()];
    for e in entries {
        if e.index < head.len() && !seen[e.index] {
            seen[e.index] = true;
            let score = e.score.clamp(0.0, 1.0);
            let highlight = e
                .highlight
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty());
            scored.push((e.index, score, highlight));
        }
    }
    // Stable sort by descending score so equal scores keep model order.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Move head results into an Option buffer so we can take them by index.
    let mut slots: Vec<Option<SearchResult>> = head.into_iter().map(Some).collect();
    let mut out: Vec<SearchResult> = Vec::with_capacity(slots.len() + tail.len());
    for (idx, score, highlight) in scored {
        if let Some(mut r) = slots[idx].take() {
            r.score = Some(score);
            if let Some(h) = highlight {
                r.highlights = vec![h];
            }
            out.push(r);
        }
    }
    // Append any head results the model omitted, preserving original order.
    for slot in slots.iter_mut() {
        if let Some(r) = slot.take() {
            out.push(r);
        }
    }
    // Tail unchanged.
    out.extend(tail);
    // Renumber ranks 1-based to reflect the new order.
    for (i, r) in out.iter_mut().enumerate() {
        r.rank = (i as u32) + 1;
    }
    out
}

fn strip_code_fences(body: &str) -> String {
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

/// Wraps an inner [`SearchProvider`] and reranks its top-N results. Transparent
/// to callers and to the edge cache (reranked results carry their score +
/// highlights through serialization).
pub struct RerankingSearchProvider {
    inner: Arc<dyn SearchProvider>,
    reranker: Arc<dyn SearchReranker>,
    top_n: usize,
}

impl RerankingSearchProvider {
    /// `top_n` is how many leading results to rerank. 0 disables reranking
    /// (acts as a pass-through), which keeps the wrapper safe to construct
    /// unconditionally.
    pub fn new(
        inner: Arc<dyn SearchProvider>,
        reranker: Arc<dyn SearchReranker>,
        top_n: usize,
    ) -> Self {
        Self {
            inner,
            reranker,
            top_n,
        }
    }
}

#[async_trait]
impl SearchProvider for RerankingSearchProvider {
    async fn search(&self, query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        let results = self.inner.search(query, opts).await?;
        if self.top_n == 0 || results.len() < 2 {
            return Ok(results);
        }
        Ok(self.reranker.rerank(query, results, self.top_n).await)
    }

    fn name(&self) -> &str {
        "reranked"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(url: &str, rank: u32) -> SearchResult {
        SearchResult {
            url: url.into(),
            rank,
            provider: "test".into(),
            ..Default::default()
        }
    }

    #[test]
    fn apply_rerank_reorders_by_score_and_attaches_highlight() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3)];
        let entries = vec![
            RerankEntry {
                index: 2,
                score: 0.9,
                highlight: Some("best match".into()),
            },
            RerankEntry {
                index: 0,
                score: 0.5,
                highlight: None,
            },
            RerankEntry {
                index: 1,
                score: 0.1,
                highlight: Some("".into()),
            },
        ];
        let out = apply_rerank(results, entries, 3);
        assert_eq!(out[0].url, "c");
        assert_eq!(out[0].rank, 1);
        assert_eq!(out[0].score, Some(0.9));
        assert_eq!(out[0].highlights, vec!["best match".to_string()]);
        assert_eq!(out[1].url, "a");
        assert_eq!(out[2].url, "b");
        // Empty highlight is dropped.
        assert!(out[2].highlights.is_empty());
    }

    #[test]
    fn apply_rerank_keeps_tail_untouched() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3), sr("d", 4)];
        // Only rerank top 2; flip them.
        let entries = vec![
            RerankEntry {
                index: 1,
                score: 0.9,
                highlight: None,
            },
            RerankEntry {
                index: 0,
                score: 0.2,
                highlight: None,
            },
        ];
        let out = apply_rerank(results, entries, 2);
        assert_eq!(out[0].url, "b");
        assert_eq!(out[1].url, "a");
        // Tail preserved in original order, no scores.
        assert_eq!(out[2].url, "c");
        assert_eq!(out[3].url, "d");
        assert!(out[2].score.is_none());
    }

    #[test]
    fn apply_rerank_omitted_head_results_trail_scored() {
        let results = vec![sr("a", 1), sr("b", 2), sr("c", 3)];
        // Model only scored index 2.
        let entries = vec![RerankEntry {
            index: 2,
            score: 0.8,
            highlight: None,
        }];
        let out = apply_rerank(results, entries, 3);
        assert_eq!(out[0].url, "c");
        // a, b omitted by model → trail in original order.
        assert_eq!(out[1].url, "a");
        assert_eq!(out[2].url, "b");
    }

    #[test]
    fn apply_rerank_ignores_out_of_range_and_duplicate_indices() {
        let results = vec![sr("a", 1), sr("b", 2)];
        let entries = vec![
            RerankEntry {
                index: 99,
                score: 1.0,
                highlight: None,
            },
            RerankEntry {
                index: 1,
                score: 0.7,
                highlight: None,
            },
            RerankEntry {
                index: 1,
                score: 0.1,
                highlight: None,
            },
        ];
        let out = apply_rerank(results, entries, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "b");
        assert_eq!(out[0].score, Some(0.7));
        assert_eq!(out[1].url, "a");
    }

    #[test]
    fn strip_code_fences_handles_json_fence() {
        assert_eq!(
            strip_code_fences("```json\n[{\"index\":0}]\n```"),
            "[{\"index\":0}]"
        );
    }

    #[tokio::test]
    async fn provider_passes_through_when_top_n_zero() {
        struct Inner;
        #[async_trait]
        impl SearchProvider for Inner {
            async fn search(
                &self,
                _q: &str,
                _o: &SearchOptions,
            ) -> QuarryResult<Vec<SearchResult>> {
                Ok(vec![sr("a", 1), sr("b", 2)])
            }
            fn name(&self) -> &str {
                "inner"
            }
        }
        struct PanicReranker;
        #[async_trait]
        impl SearchReranker for PanicReranker {
            async fn rerank(
                &self,
                _q: &str,
                _r: Vec<SearchResult>,
                _n: usize,
            ) -> Vec<SearchResult> {
                panic!("must not be called when top_n=0");
            }
        }
        let p = RerankingSearchProvider::new(Arc::new(Inner), Arc::new(PanicReranker), 0);
        let out = p.search("q", &SearchOptions::default()).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "a");
    }
}
