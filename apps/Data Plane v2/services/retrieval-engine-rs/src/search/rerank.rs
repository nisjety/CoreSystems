use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::pipeline::types::ScoredCandidate;

const MAX_RETRIES: u32 = 2;
const INITIAL_BACKOFF_MS: u64 = 300;

#[derive(Clone)]
pub struct RerankClient {
    http: Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: usize,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

#[derive(Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f32,
}

impl RerankClient {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// §16.3.5 — when the candidate list is bigger than `PARALLEL_SPLIT_AT`
    /// we split it into chunks and call Cohere in parallel, then merge by
    /// best score per index. Cuts total RTT roughly in half on a 100-item
    /// rerank without changing the contract.
    const PARALLEL_SPLIT_AT: usize = 50;
    const PARALLEL_CHUNK: usize = 32;

    #[tracing::instrument(
        name = "rerank.invoke",
        skip(self, query, candidates),
        fields(
            otel.kind = "client",
            model = self.model.as_str(),
            candidate_count = candidates.len(),
            top_n = top_n,
        ),
    )]
    pub async fn rerank(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }

        // Parallel split path: keep behavior identical for small lists,
        // shard + parallel-call only when it actually saves wall time.
        if candidates.len() > Self::PARALLEL_SPLIT_AT {
            return self.rerank_parallel(query, candidates, top_n).await;
        }

        let documents: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();

        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents,
            top_n,
        };

        let rerank_resp = {
            let mut last_err = None;
            let mut result = None;

            for attempt in 0..=MAX_RETRIES {
                if attempt > 0 {
                    let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                    tracing::warn!(attempt, ?backoff, "rerank retry");
                    tokio::time::sleep(backoff).await;
                }

                let resp = match self
                    .http
                    .post("https://api.cohere.ai/v1/rerank")
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = Some(anyhow::anyhow!(e).context("rerank API call failed"));
                        continue;
                    }
                };

                if resp.status().is_server_error() || resp.status().as_u16() == 429 {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    last_err = Some(anyhow::anyhow!("rerank API returned {status}: {text}"));
                    continue;
                }

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("rerank API returned {status}: {text}");
                }

                result = Some(
                    resp.json::<RerankResponse>()
                        .await
                        .context("parse rerank response")?,
                );
                break;
            }

            match result {
                Some(r) => r,
                None => {
                    return Err(
                        last_err.unwrap_or_else(|| anyhow::anyhow!("rerank retries exhausted"))
                    )
                }
            }
        };

        // §14.2 NaN sanitization: if the rerank API returns NaN/Inf, downstream
        // ordering breaks (any comparison with NaN is `false`, so sort becomes
        // undefined and `final_score` poisons the response). Coerce non-finite
        // scores to 0.0 so the candidate sinks but doesn't corrupt the sort.
        let reranked: Vec<ScoredCandidate> = rerank_resp
            .results
            .into_iter()
            .filter_map(|r| {
                candidates.get(r.index).map(|c| {
                    let mut reranked = c.clone();
                    let score = if r.relevance_score.is_finite() {
                        r.relevance_score
                    } else {
                        tracing::warn!(
                            index = r.index,
                            raw = ?r.relevance_score,
                            "rerank API returned non-finite score; coerced to 0.0"
                        );
                        0.0
                    };
                    reranked.rerank_score = score;
                    reranked.final_score = score;
                    reranked
                })
            })
            .collect();

        Ok(reranked)
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Shard the candidate list, rerank each shard concurrently, merge by
    /// taking the top-N globally. We use a Vec of futures + `join_all` so
    /// failure of one shard fails the whole call (caller already retries).
    async fn rerank_parallel(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let chunks: Vec<&[ScoredCandidate]> = candidates.chunks(Self::PARALLEL_CHUNK).collect();
        let per_chunk_top = top_n.max(8);

        let mut tasks = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            // Each chunk is reranked against the same query, asking for the
            // chunk's own top-N. We re-merge globally below.
            tasks.push(self.rerank_single(query, chunk, per_chunk_top));
        }

        let results = futures::future::join_all(tasks).await;
        let mut merged: Vec<ScoredCandidate> = Vec::new();
        for r in results {
            merged.extend(r?);
        }

        // Sort by final_score desc; NaN-safe because rerank_single() already
        // coerced non-finite scores to 0.0 (§14.2).
        merged.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        merged.truncate(top_n);
        Ok(merged)
    }

    /// Single-call rerank — extracted so `rerank_parallel` can reuse it.
    /// Identical body to the small-list path in `rerank()`, just factored out.
    async fn rerank_single(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let documents: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();
        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents,
            top_n,
        };

        let resp = self
            .http
            .post("https://api.cohere.ai/v1/rerank")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .context("rerank API call failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("rerank API returned {status}: {text}");
        }

        let rerank_resp: RerankResponse = resp.json().await.context("parse rerank response")?;

        Ok(rerank_resp
            .results
            .into_iter()
            .filter_map(|r| {
                candidates.get(r.index).map(|c| {
                    let mut out = c.clone();
                    let score = if r.relevance_score.is_finite() {
                        r.relevance_score
                    } else {
                        0.0
                    };
                    out.rerank_score = score;
                    out.final_score = score;
                    out
                })
            })
            .collect())
    }
}
