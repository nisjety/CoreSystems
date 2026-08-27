use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::pipeline::types::ScoredCandidate;

const MAX_RETRIES: u32 = 2;
/// First backoff step; doubles per attempt (1s, then 2s).
///
/// Was 300ms, which is why the reranker was effectively dead under load: the
/// Azure Foundry S0 tier answers 429 with `Retry-After: 1` and a per-minute
/// window, so retries at +300ms/+600ms all landed inside the same window and
/// exhausted. Because rerank failure deliberately degrades to fused order, the
/// only visible symptom was every candidate carrying `rerank_score: 0.0` —
/// measured live at a 23% failure rate on light probe traffic, and worse in
/// bursts. Worst-case added latency is 1s+2s=3s on a rate-limited query, which
/// is the price of the cross-encoder actually running; a chronically limited
/// deployment still degrades non-fatally exactly as before.
const INITIAL_BACKOFF_MS: u64 = 1_000;
/// Upper bound on a provider-supplied `Retry-After`, so a hostile or confused
/// upstream cannot pin a retrieval request for a minute.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

/// Provider-directed backoff: prefer the 429's own `Retry-After` (seconds
/// form) over the local schedule, since the provider knows its window and the
/// local guess is what made retries useless before. Capped, and only ever
/// LENGTHENS the local backoff — a `Retry-After: 0` must not turn the retry
/// into a same-window hammer.
fn backoff_for(attempt: u32, retry_after: Option<Duration>) -> Duration {
    let local = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt.saturating_sub(1)));
    match retry_after {
        Some(hinted) => hinted.min(MAX_RETRY_AFTER).max(local),
        None => local,
    }
}

fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[derive(Clone)]
pub struct RerankClient {
    http: Client,
    api_key: String,
    model: String,
    /// Full rerank URL. Defaults to public Cohere; set to an Azure AI Foundry
    /// serverless Cohere rerank endpoint (`https://<deployment>.<region>.models.ai.azure.com/v2/rerank`)
    /// to use the in-EU deployment instead of the public API.
    endpoint: String,
    /// Auth header style: `true` → `Authorization: Bearer <key>` (public Cohere);
    /// `false` → `api-key: <key>` (Azure Foundry).
    use_bearer: bool,
}

#[cfg(test)]
mod security_tests {
    use super::sanitized_rerank_status_error;

    #[test]
    fn provider_error_does_not_include_response_body() {
        let error = sanitized_rerank_status_error(reqwest::StatusCode::BAD_GATEWAY).to_string();
        assert_eq!(error, "rerank API returned HTTP status 502");
        assert!(!error.contains("document"));
    }
}

#[cfg(test)]
mod backoff_tests {
    use super::{backoff_for, MAX_RETRY_AFTER};
    use std::time::Duration;

    // Regression: the local schedule must clear the provider's advertised
    // window. Azure Foundry S0 answers 429 with `Retry-After: 1`; the old
    // 300ms/600ms schedule retried inside the same window and exhausted, which
    // — because rerank failure is non-fatal by design — silently disabled the
    // cross-encoder (23% of probe traffic degraded to fused order).
    #[test]
    fn local_schedule_clears_a_one_second_rate_window() {
        assert!(backoff_for(1, None) >= Duration::from_secs(1));
        assert_eq!(backoff_for(2, None), Duration::from_secs(2));
    }

    #[test]
    fn provider_retry_after_lengthens_but_never_shortens() {
        // The provider knows its window better than the local guess does.
        assert_eq!(
            backoff_for(1, Some(Duration::from_secs(3))),
            Duration::from_secs(3)
        );
        // A `Retry-After: 0` must not turn the retry into a same-window hammer.
        assert_eq!(
            backoff_for(1, Some(Duration::ZERO)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn provider_retry_after_is_capped() {
        // A hostile or confused upstream cannot pin a retrieval request.
        assert_eq!(
            backoff_for(1, Some(Duration::from_secs(3600))),
            MAX_RETRY_AFTER
        );
    }
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
    #[allow(dead_code)]
    pub fn new(api_key: &str, model: &str) -> Self {
        Self::with_endpoint(api_key, model, "https://api.cohere.ai/v1/rerank", true)
    }

    /// Build with an explicit endpoint + auth style. An empty `endpoint` falls
    /// back to public Cohere.
    ///
    /// `use_bearer` is honored exactly as given -- there is deliberately NO
    /// endpoint sniffing here. An earlier version of this doc claimed an
    /// `*.azure.com` endpoint "auto-selects the api-key header", which the code
    /// never did; that false promise is why nobody noticed `RERANK_ENDPOINT` was
    /// unset. Both `api-key` and `Authorization: Bearer` are in fact accepted by
    /// Azure AI Foundry's Cohere rerank route (verified against the live
    /// deployment), so the auth style was never the failure -- sending an Azure
    /// key to the *public Cohere* default URL was, and that returns 401.
    pub fn with_endpoint(api_key: &str, model: &str, endpoint: &str, use_bearer: bool) -> Self {
        let endpoint = if endpoint.trim().is_empty() {
            "https://api.cohere.ai/v1/rerank".to_string()
        } else {
            endpoint.trim().to_string()
        };
        Self {
            http: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            endpoint,
            use_bearer,
        }
    }

    /// POST builder with the configured URL + auth header.
    fn rerank_post(&self) -> reqwest::RequestBuilder {
        let rb = self.http.post(&self.endpoint);
        if self.use_bearer {
            rb.header("Authorization", format!("Bearer {}", self.api_key))
        } else {
            rb.header("api-key", &self.api_key)
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

        let rerank_resp = self.send_with_retry(&body, Duration::ZERO).await?;

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

    /// The one place a rerank HTTP call is made, with 429/5xx retry.
    ///
    /// Extracted because it used to exist only inline in the small-list path,
    /// while `rerank_single` — the sharded path, which is the COMMON one, since
    /// six fused arms routinely exceed `PARALLEL_SPLIT_AT` candidates — sent
    /// bare requests. One 429 on any shard failed the whole rerank, and because
    /// rerank failure deliberately degrades to fused order, the cross-encoder
    /// was silently absent from most production queries (measured: 91 of 103
    /// retrievals degraded during one eval window, all 429).
    ///
    /// `stagger` delays the FIRST attempt: `rerank_parallel` fires shards
    /// concurrently, and against a per-second rate window simultaneous shards
    /// guarantee that all but one 429 on arrival and then retry in lockstep.
    /// Offsetting each shard's start spreads them across the window instead.
    async fn send_with_retry(
        &self,
        body: &RerankRequest,
        stagger: Duration,
    ) -> anyhow::Result<RerankResponse> {
        if !stagger.is_zero() {
            tokio::time::sleep(stagger).await;
        }
        let mut last_err = None;
        let mut retry_after: Option<Duration> = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = backoff_for(attempt, retry_after.take());
                tracing::warn!(attempt, ?backoff, "rerank retry");
                tokio::time::sleep(backoff).await;
            }

            let resp = match self.rerank_post().json(body).send().await {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(anyhow::anyhow!(e).context("rerank API call failed"));
                    continue;
                }
            };

            if resp.status().is_server_error() || resp.status().as_u16() == 429 {
                let status = resp.status();
                retry_after = parse_retry_after(&resp);
                last_err = Some(sanitized_rerank_status_error(status));
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                return Err(sanitized_rerank_status_error(status));
            }

            return resp
                .json::<RerankResponse>()
                .await
                .context("parse rerank response");
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("rerank retries exhausted")))
    }

    /// Shard the candidate list, rerank each shard concurrently, merge by
    /// taking the top-N globally. A Vec of futures + `join_all`, so failure of
    /// one shard fails the whole call — retries live INSIDE each shard's
    /// `send_with_retry`; nothing above this retries (the postprocess stage
    /// deliberately degrades to fused order instead).
    async fn rerank_parallel(
        &self,
        query: &str,
        candidates: &[ScoredCandidate],
        top_n: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let chunks: Vec<&[ScoredCandidate]> = candidates.chunks(Self::PARALLEL_CHUNK).collect();
        let per_chunk_top = top_n.max(8);

        let mut tasks = Vec::with_capacity(chunks.len());
        for (i, chunk) in chunks.into_iter().enumerate() {
            // Each chunk is reranked against the same query, asking for the
            // chunk's own top-N. We re-merge globally below.
            //
            // Staggered starts: simultaneous shards against a per-second rate
            // window guarantee all but one 429 on arrival, then retry in
            // lockstep and 429 again — the split's whole latency benefit spent
            // on synchronized failure. The offset spreads shard arrivals across
            // the window; on an unthrottled provider it costs at most
            // (shards-1) x 300ms, bounded by the small shard count.
            tasks.push(self.rerank_single(
                query,
                chunk,
                per_chunk_top,
                Duration::from_millis(300) * i as u32,
            ));
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
        stagger: Duration,
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

        let rerank_resp = self.send_with_retry(&body, stagger).await?;

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

fn sanitized_rerank_status_error(status: reqwest::StatusCode) -> anyhow::Error {
    anyhow::anyhow!("rerank API returned HTTP status {}", status.as_u16())
}
