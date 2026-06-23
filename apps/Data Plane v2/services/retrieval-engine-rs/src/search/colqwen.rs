//! ColQwen visual reranker client (late-interaction MaxSim).
//!
//! ColQwen2.5 / Qwen3-VL-Embedding runs as a separate GPU inference server
//! (local for verification; Hetzner/Azure for production). This client POSTs the
//! query plus the candidate page-image URLs and receives one MaxSim relevance
//! score per image, which the orchestrator uses to reorder Embed-v4's visual
//! top-K. Reranking refines ordering only — every call site treats failures as
//! non-fatal and degrades to the Embed-v4 order.
//!
//! Wire contract (host-independent; same server image local or on Hetzner):
//! ```text
//! POST {endpoint}/rerank
//! { "query": "...", "image_urls": ["https://.../p0", "https://.../p1"] }
//! → { "scores": [0.83, 0.41] }   // one MaxSim score per image_url, in order
//! ```

use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ColqwenClient {
    http: Client,
    endpoint: String,
}

// Manual Debug: keep the struct introspectable without implying any secret state.
impl std::fmt::Debug for ColqwenClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColqwenClient")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

#[derive(Serialize)]
struct RerankRequest<'a> {
    query: &'a str,
    image_urls: &'a [String],
}

#[derive(Deserialize)]
struct RerankResponse {
    scores: Vec<f32>,
}

impl ColqwenClient {
    /// Build from a base URL (e.g. `http://host.docker.internal:8090`). Returns
    /// `None` when the URL is empty, so a misconfigured/disabled endpoint simply
    /// turns the visual reranker off rather than erroring.
    pub fn from_url(endpoint: &str) -> Option<Self> {
        let endpoint = endpoint.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            return None;
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            http,
            endpoint: endpoint.to_string(),
        })
    }

    /// Return one MaxSim relevance score per `image_urls` entry, in order.
    pub async fn rerank(&self, query: &str, image_urls: &[String]) -> anyhow::Result<Vec<f32>> {
        if image_urls.is_empty() {
            return Ok(vec![]);
        }
        let url = format!("{}/rerank", self.endpoint);
        let resp = self
            .http
            .post(&url)
            .json(&RerankRequest { query, image_urls })
            .send()
            .await
            .context("colqwen rerank request")?
            .error_for_status()
            .context("colqwen rerank status")?;
        let parsed: RerankResponse = resp.json().await.context("colqwen rerank parse")?;
        if parsed.scores.len() != image_urls.len() {
            anyhow::bail!(
                "colqwen score count mismatch: got {}, want {}",
                parsed.scores.len(),
                image_urls.len()
            );
        }
        Ok(parsed.scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_url_empty_is_none_and_trims_slash() {
        assert!(ColqwenClient::from_url("").is_none());
        assert!(ColqwenClient::from_url("   ").is_none());
        let c = ColqwenClient::from_url("http://h:8090/").expect("built");
        assert_eq!(c.endpoint, "http://h:8090");
    }
}
