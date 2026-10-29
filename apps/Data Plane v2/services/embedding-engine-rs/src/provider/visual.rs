//! Visual (multimodal) embeddings via **Cohere Embed v4** on Azure AI Foundry.
//!
//! Embeds rendered document-page images — plus an optional fused title/caption —
//! into a single dense vector per page (default 1536-dim) for the visual RAG arm.
//! The page-image consumer (PR-D) calls [`VisualEmbeddingProvider::embed_images`]
//! and upserts the vectors into the `dataplane_page_images` Qdrant collection,
//! which retrieval fuses via `w_visual`.
//!
//! Wire contract — Foundry "image embeddings" (`api-version=2024-05-01-preview`):
//! ```text
//! POST {endpoint}/images/embeddings?api-version=2024-05-01-preview
//! api-key: <key>            (or Authorization: Bearer <token>, scope https://ai.azure.com/.default)
//! Content-Type: application/json
//! { "model": "Cohere-embed-4",
//!   "input": [ { "image": "data:image/png;base64,…", "text": "<optional>" } ],
//!   "input_type": "document" }          // "query" at search time
//! → { "data": [ { "embedding": [ … ] } ], "model": "…", "usage": {…} }
//! ```
//! Docs: <https://learn.microsoft.com/azure/ai-foundry/model-inference/how-to/use-image-embeddings>
//!
//! Unlike the text providers in this crate, Embed v4 is multimodal and
//! single-vector, so its output drops into the existing single-vector cosine
//! Qdrant pattern with no multivector / MAX_SIM machinery.

use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 500;

/// One image input: a base64 image **data URI** (`data:image/png;base64,…`).
///
/// `text` exists for API completeness, but the Azure Foundry `embed-v-4-0`
/// deployment rejects an input that carries **both** `image` and `text`
/// (HTTP 422 "cannot have both text and image inputs"). The page-image
/// consumer therefore leaves `text` `None` and keeps any title in the Qdrant
/// payload instead of fusing it into the vector.
#[derive(Clone, Debug)]
pub struct ImageEmbedInput {
    pub image_data_url: String,
    pub text: Option<String>,
}

/// Cohere Embed v4 multimodal embedding client (Azure AI Foundry MaaS).
#[derive(Clone)]
pub struct VisualEmbeddingProvider {
    http: Client,
    endpoint: String,
    api_key: String,
    model: String,
    api_version: String,
}

// Manual Debug so the API key is never printed (a derived impl would leak it).
impl std::fmt::Debug for VisualEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualEmbeddingProvider")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_version", &self.api_version)
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct VisualInputItem<'a> {
    image: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
}

#[derive(Serialize)]
struct VisualEmbedRequest<'a> {
    model: &'a str,
    input: Vec<VisualInputItem<'a>>,
    input_type: &'a str,
}

#[derive(Deserialize)]
struct VisualEmbedResponse {
    data: Vec<VisualEmbedDatum>,
}

#[derive(Deserialize)]
struct VisualEmbedDatum {
    embedding: Vec<f32>,
}

impl VisualEmbeddingProvider {
    /// Build from config. Returns `Ok(None)` when no Embed v4 endpoint is
    /// configured — the visual arm is simply disabled, so text-only deployments
    /// don't error. Bails if an endpoint is set without an API key.
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
                .timeout(Duration::from_secs(60))
                .build()
                .context("build visual embedding HTTP client")?,
            endpoint: cfg
                .cohere_embed_v4_endpoint
                .trim_end_matches('/')
                .to_string(),
            api_key: cfg.cohere_embed_v4_api_key.clone(),
            model: cfg.cohere_embed_v4_deployment.clone(),
            api_version: cfg.cohere_embed_v4_api_version.clone(),
        }))
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Embed rendered page images for storage (`input_type: "document"`).
    /// Returns one vector per input, in order. Empty input → empty result.
    pub async fn embed_images(
        &self,
        inputs: &[ImageEmbedInput],
        zdr: bool,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        // ZDR egress guard: Azure Foundry is a *retaining* provider, so page
        // images classified restricted (Zero Data Retention) must never leave on
        // it. Fail closed BEFORE any network call — mirrors the direct-Azure text
        // path. (A ZDR-compliant visual embedding path is a later prerequisite;
        // until then restricted pages have no compliant visual arm.)
        if zdr {
            anyhow::bail!("ZDR content must not egress to the Cohere Embed v4 visual path");
        }

        let url = format!(
            "{}/images/embeddings?api-version={}",
            self.endpoint, self.api_version
        );
        let body = VisualEmbedRequest {
            model: &self.model,
            input: inputs
                .iter()
                .map(|i| VisualInputItem {
                    image: &i.image_data_url,
                    text: i.text.as_deref(),
                })
                .collect(),
            input_type: "document",
        };

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    INITIAL_BACKOFF_MS * 2u64.pow(attempt),
                ))
                .await;
                tracing::warn!(attempt, "retrying visual embedding API call");
            }

            match self
                .http
                .post(&url)
                .header("api-key", &self.api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let parsed: VisualEmbedResponse = resp
                        .json()
                        .await
                        .context("parse visual embedding response")?;
                    if parsed.data.len() != inputs.len() {
                        anyhow::bail!(
                            "visual embedding count mismatch: got {}, want {}",
                            parsed.data.len(),
                            inputs.len()
                        );
                    }
                    return Ok(parsed.data.into_iter().map(|d| d.embedding).collect());
                }
                Ok(resp) if resp.status().as_u16() == 429 || resp.status().is_server_error() => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    tracing::warn!(%status, body = %body_text, "visual embedding retryable failure");
                    last_err = Some(anyhow::anyhow!(
                        "visual embedding API {status}: {body_text}"
                    ));
                    continue;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("visual embedding API {status}: {body_text}");
                }
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("visual embedding failed after retries")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(endpoint: &str, key: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "",
            "nats_url": "",
            "qdrant_url": "",
            "cohere_embed_v4_endpoint": endpoint,
            "cohere_embed_v4_api_key": key,
        }))
        .expect("minimal config from defaults")
    }

    fn test_provider(endpoint: &str) -> VisualEmbeddingProvider {
        VisualEmbeddingProvider {
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
        assert!(VisualEmbeddingProvider::from_config(&cfg_with("", ""))
            .unwrap()
            .is_none());
    }

    #[test]
    fn from_config_requires_key_when_endpoint_set() {
        let err =
            VisualEmbeddingProvider::from_config(&cfg_with("https://x.services.ai.azure.com", ""))
                .unwrap_err();
        assert!(
            err.to_string().contains("COHERE_EMBED_V4_API_KEY"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn from_config_builds_and_trims_trailing_slash() {
        let provider = VisualEmbeddingProvider::from_config(&cfg_with(
            "https://x.services.ai.azure.com/",
            "k",
        ))
        .unwrap()
        .expect("provider configured");
        assert_eq!(provider.model_name(), "Cohere-embed-4");
        assert_eq!(provider.endpoint, "https://x.services.ai.azure.com");
    }

    /// Pins the Foundry image-embeddings request shape: `text` is omitted when
    /// `None`, `input_type` is `document`, and each item carries the data-URI.
    #[test]
    fn request_serializes_to_foundry_image_embeddings_shape() {
        let req = VisualEmbedRequest {
            model: "Cohere-embed-4",
            input: vec![
                VisualInputItem {
                    image: "data:image/png;base64,AAAA",
                    text: Some("Page 1 — Q4 results"),
                },
                VisualInputItem {
                    image: "data:image/png;base64,BBBB",
                    text: None,
                },
            ],
            input_type: "document",
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "Cohere-embed-4");
        assert_eq!(v["input_type"], "document");
        assert_eq!(v["input"][0]["image"], "data:image/png;base64,AAAA");
        assert_eq!(v["input"][0]["text"], "Page 1 — Q4 results");
        assert_eq!(v["input"][1]["image"], "data:image/png;base64,BBBB");
        assert!(
            v["input"][1].get("text").is_none(),
            "text must be omitted when None"
        );
    }

    /// ZDR egress guard: a restricted page image must fail closed BEFORE any
    /// network call (the unreachable endpoint would surface a connection error if
    /// the guard regressed, not the ZDR error asserted here).
    #[tokio::test]
    async fn embed_images_egress_guard_rejects_zdr() {
        let provider = test_provider("http://127.0.0.1:1/unreachable");
        let err = provider
            .embed_images(
                &[ImageEmbedInput {
                    image_data_url: "data:image/png;base64,AAAA".into(),
                    text: None,
                }],
                true,
            )
            .await
            .expect_err("restricted (ZDR) page must not egress to Embed v4");
        assert!(
            err.to_string()
                .contains("must not egress to the Cohere Embed v4 visual path"),
            "unexpected error: {err}"
        );
    }

    /// Non-restricted pages still embed (guard trips only for ZDR=true). The
    /// unreachable endpoint yields a network error, NOT the egress error —
    /// proving the guard is not constant-on.
    #[tokio::test]
    async fn embed_images_allows_non_zdr() {
        let provider = test_provider("http://127.0.0.1:1/unreachable");
        let err = provider
            .embed_images(
                &[ImageEmbedInput {
                    image_data_url: "data:image/png;base64,AAAA".into(),
                    text: None,
                }],
                false,
            )
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the Cohere Embed v4 visual path"),
            "non-ZDR content must not hit the egress guard: {err}"
        );
    }

    #[tokio::test]
    async fn embed_images_empty_input_is_noop() {
        let provider = test_provider("http://127.0.0.1:1/unreachable");
        let out = provider.embed_images(&[], false).await.unwrap();
        assert!(out.is_empty());
    }
}
