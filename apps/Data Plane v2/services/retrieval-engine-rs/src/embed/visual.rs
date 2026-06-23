//! Visual-arm **query** embeddings via Cohere Embed v4 (Azure AI Foundry).
//!
//! The index side (embedding-engine) embeds page *images* into the
//! `dataplane_page_images` collection. To search it we embed the TEXT query into
//! Embed v4's *same* multimodal space (`input_type: "query"`) — Embed v4 unifies
//! text and image, so a text-query vector is directly comparable to the stored
//! page-image vectors. This is a separate, single-purpose client from the dense
//! text embedder (`EmbeddingClient`), which targets `text-embedding-3-large`.
//!
//! Text route (Foundry Model Inference, `api-version=2024-05-01-preview`):
//! ```text
//! POST {endpoint}/embeddings?api-version=2024-05-01-preview
//! api-key: <key>
//! { "model": "Cohere-embed-4", "input": ["<query>"], "input_type": "query" }
//! → { "data": [ { "embedding": [ … ] } ] }
//! ```
//! Docs: <https://learn.microsoft.com/azure/ai-foundry/model-inference/how-to/use-image-embeddings>

use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Clone)]
pub struct VisualQueryEmbedder {
    http: Client,
    endpoint: String,
    api_key: String,
    model: String,
    api_version: String,
}

// Manual Debug so the API key is never printed (a derived impl would leak it).
impl std::fmt::Debug for VisualQueryEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualQueryEmbedder")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_version", &self.api_version)
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct QueryEmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
    input_type: &'a str,
}

#[derive(Deserialize)]
struct QueryEmbedResponse {
    data: Vec<QueryEmbedDatum>,
}

#[derive(Deserialize)]
struct QueryEmbedDatum {
    embedding: Vec<f32>,
}

impl VisualQueryEmbedder {
    /// Build from config. `Ok(None)` when no Embed v4 endpoint is configured —
    /// the visual arm is simply dark. Bails if an endpoint is set without a key.
    pub fn from_config(cfg: &Config) -> anyhow::Result<Option<Self>> {
        if cfg.cohere_embed_v4_endpoint.trim().is_empty() {
            return Ok(None);
        }
        if cfg.cohere_embed_v4_api_key.trim().is_empty() {
            anyhow::bail!(
                "COHERE_EMBED_V4_API_KEY is required when COHERE_EMBED_V4_ENDPOINT is set"
            );
        }
        Ok(Some(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .context("build visual query embed client")?,
            endpoint: cfg
                .cohere_embed_v4_endpoint
                .trim_end_matches('/')
                .to_string(),
            api_key: cfg.cohere_embed_v4_api_key.clone(),
            model: cfg.cohere_embed_v4_deployment.clone(),
            api_version: cfg.cohere_embed_v4_api_version.clone(),
        }))
    }

    /// Embed a text query into Embed v4's multimodal space (`input_type: query`).
    /// Fails closed for ZDR queries (Embed v4 retains) — the caller treats the
    /// error as "skip the visual arm", exactly like a transient ANN failure.
    pub async fn embed_query(&self, text: &str, zdr: bool) -> anyhow::Result<Vec<f32>> {
        if zdr {
            anyhow::bail!("ZDR query must not egress to the Cohere Embed v4 visual path");
        }
        let url = format!("{}/embeddings?api-version={}", self.endpoint, self.api_version);
        let body = QueryEmbedRequest {
            model: &self.model,
            input: vec![text],
            input_type: "query",
        };
        let resp = self
            .http
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("visual query embed call")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("visual query embed API {status}: {body_text}");
        }
        let parsed: QueryEmbedResponse = resp
            .json()
            .await
            .context("parse visual query embed response")?;
        parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .context("empty visual query embed response")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(endpoint: &str, key: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "",
            "qdrant_url": "",
            "cohere_embed_v4_endpoint": endpoint,
            "cohere_embed_v4_api_key": key,
        }))
        .expect("minimal config from defaults")
    }

    fn test_embedder(endpoint: &str) -> VisualQueryEmbedder {
        VisualQueryEmbedder {
            http: Client::builder()
                .timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key: "fake-key".into(),
            model: "Cohere-embed-4".into(),
            api_version: "2024-05-01-preview".into(),
        }
    }

    #[test]
    fn from_config_none_when_unconfigured() {
        assert!(VisualQueryEmbedder::from_config(&cfg_with("", ""))
            .unwrap()
            .is_none());
    }

    #[test]
    fn from_config_requires_key_when_endpoint_set() {
        let err =
            VisualQueryEmbedder::from_config(&cfg_with("https://x.services.ai.azure.com", ""))
                .unwrap_err();
        assert!(
            err.to_string().contains("COHERE_EMBED_V4_API_KEY"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn from_config_builds_and_trims_trailing_slash() {
        let embedder =
            VisualQueryEmbedder::from_config(&cfg_with("https://x.services.ai.azure.com/", "k"))
                .unwrap()
                .expect("embedder configured");
        assert_eq!(embedder.endpoint, "https://x.services.ai.azure.com");
        assert_eq!(embedder.model, "Cohere-embed-4");
    }

    /// Pins the query request shape: `input` is a string array and `input_type`
    /// is `query` (so the vector lands in the same space as stored page images).
    #[test]
    fn query_request_serializes_to_expected_shape() {
        let req = QueryEmbedRequest {
            model: "Cohere-embed-4",
            input: vec!["quarterly revenue table"],
            input_type: "query",
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "Cohere-embed-4");
        assert_eq!(v["input_type"], "query");
        assert_eq!(v["input"][0], "quarterly revenue table");
    }

    /// ZDR query must fail closed BEFORE any network call (unreachable endpoint
    /// would surface a connection error if the guard regressed).
    #[tokio::test]
    async fn embed_query_egress_guard_rejects_zdr() {
        let embedder = test_embedder("http://127.0.0.1:1/unreachable");
        let err = embedder
            .embed_query("secret query", true)
            .await
            .expect_err("ZDR query must not egress to Embed v4");
        assert!(
            err.to_string()
                .contains("must not egress to the Cohere Embed v4 visual path"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn embed_query_allows_non_zdr() {
        let embedder = test_embedder("http://127.0.0.1:1/unreachable");
        let err = embedder
            .embed_query("public query", false)
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the Cohere Embed v4 visual path"),
            "non-ZDR query must not hit the egress guard: {err}"
        );
    }
}
