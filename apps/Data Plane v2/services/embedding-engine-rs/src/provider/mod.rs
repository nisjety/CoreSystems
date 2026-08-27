use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;

pub mod contextualize;
mod inference_auth;
pub mod media;
pub mod video_caption;
pub mod visual;
use inference_auth::{InferenceTokenClient, RetentionPosture};

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 500;

mod model_plane {
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

#[derive(Clone)]
pub struct EmbeddingProvider {
    inner: EmbeddingBackend,
}

#[derive(Clone)]
enum EmbeddingBackend {
    // Boxed: the model-plane client (gRPC channel + config incl. the residency
    // region) is the larger variant; boxing keeps `EmbeddingBackend` small and
    // satisfies `clippy::large_enum_variant`.
    ModelPlane(Box<ModelPlaneEmbeddingClient>),
    AzureOpenAi(AzureOpenAiEmbeddingClient),
    Cohere(CohereEmbeddingClient),
}

#[derive(Clone)]
struct ModelPlaneEmbeddingClient {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    model: String,
    provider: String,
    /// Requested residency region forwarded to inference-core, which enforces
    /// the EU residency gate deny-by-default. Empty = no preference.
    region: String,
    timeout: Duration,
    inference_token_client: InferenceTokenClient,
}

#[derive(Clone)]
struct AzureOpenAiEmbeddingClient {
    http: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
}

/// Cohere Embed v4 (Azure AI Foundry) — dense **text** embeddings. Shares the
/// deployment the visual arm already uses (`provider/visual.rs`): Embed v4 is
/// one multimodal model serving both images and text, so this reuses the same
/// `cohere_embed_v4_*` credentials rather than adding a new provider config.
///
/// Wire contract — Foundry's **general** embeddings route (distinct from the
/// visual arm's `/images/embeddings`):
/// ```text
/// POST {endpoint}/embeddings?api-version=2024-05-01-preview
/// api-key: <key>
/// { "model": "Cohere-embed-4", "input": ["text1", "text2"], "input_type": "document" }
/// → { "data": [ { "embedding": [ … ] } ] }
/// ```
/// `input_type` is `"document"` for indexing (this client) and `"query"` for
/// retrieval (`retrieval-engine-rs/src/embed/mod.rs`'s mirror of this client).
#[derive(Clone)]
struct CohereEmbeddingClient {
    http: Client,
    endpoint: String,
    api_key: String,
    model: String,
    api_version: String,
    /// Matryoshka output width, sent on every request. See
    /// [`validate_matryoshka_dimension`].
    output_dimension: u32,
}

#[derive(Serialize)]
struct EmbedRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Serialize)]
struct CohereEmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    input_type: &'a str,
    /// Requested Matryoshka truncation width.
    ///
    /// MEASURED CAVEAT (2026-08-25, live `embed-v-4-0` on Azure AI Foundry):
    /// this deployment **ignores it** — 256/512/1024/1536 all return 1536. It
    /// is still sent because it is the correct parameter against Cohere's
    /// native API, and harmless here, but it must NOT be relied on and it is
    /// NOT what protects the collection/vector width invariant. The
    /// response-width check in `embed_batch` is.
    ///
    /// Sending it REQUIRES the `extra-parameters: pass-through` header; without
    /// that header Foundry 400s the entire request.
    output_dimension: u32,
}

/// Cohere Embed v4's Matryoshka-supported output widths.
///
/// Embed v4 is trained so a prefix of the full 1536-dim vector is itself a
/// usable embedding (Matryoshka representation learning), but only at these
/// four cut points.
///
/// Validating against this set stops an impossible width being configured. It
/// does NOT mean the width is honoured — the Foundry deployment measured on
/// 2026-08-25 returns 1536 for all four. Treat a non-default value as a
/// request, and let the response-width check report whether it was granted.
const COHERE_MATRYOSHKA_DIMENSIONS: [u64; 4] = [256, 512, 1024, 1536];

/// Fail closed on a dimension Embed v4 cannot produce.
///
/// Rejecting at construction (startup) rather than on first embed matters: the
/// alternative is a service that boots healthy, creates a Qdrant collection at
/// the bad width, and only fails once real documents arrive. The query side
/// (`retrieval-engine-rs`) validates the identical set, so an unusable width
/// cannot be half-adopted across the two engines.
fn validate_matryoshka_dimension(dimension: u64, env_var: &str) -> anyhow::Result<u32> {
    if !COHERE_MATRYOSHKA_DIMENSIONS.contains(&dimension) {
        anyhow::bail!(
            "{env_var}={dimension} is not a Cohere Embed v4 Matryoshka width; \
             expected one of {COHERE_MATRYOSHKA_DIMENSIONS:?}"
        );
    }
    // Every value in the set fits u32; the cast cannot truncate.
    Ok(dimension as u32)
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
}

impl EmbeddingProvider {
    pub fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        match normalize_provider(&cfg.embedding_provider).as_str() {
            "model_plane" => Self::model_plane(
                &cfg.model_plane_ai_core_grpc_url,
                &cfg.azure_openai_embedding_deployment,
                &cfg.model_plane_embedding_provider,
                &cfg.embedding_region,
                cfg.model_plane_embedding_timeout_ms,
                &cfg.model_plane_inference_token_url,
                &cfg.model_plane_inference_token_issuer,
                &cfg.model_plane_inference_service_id,
                &cfg.model_plane_inference_service_api_key,
                &cfg.model_plane_inference_retention_posture,
            ),
            "azure_openai" => Self::azure_openai(
                &cfg.azure_openai_endpoint,
                &cfg.azure_openai_api_key,
                &cfg.azure_openai_embedding_deployment,
            ),
            "cohere" => Self::cohere(
                &cfg.cohere_embed_v4_endpoint,
                &cfg.cohere_embed_v4_api_key,
                &cfg.cohere_embed_v4_deployment,
                &cfg.cohere_embed_v4_api_version,
                // Same value `main.rs` sizes the Qdrant collection with, so the
                // wire width and the collection width cannot drift apart.
                cfg.embedding_dimension,
            ),
            other => anyhow::bail!(
                "unsupported EMBEDDING_PROVIDER `{other}`; expected `model_plane`, `azure_openai`, or `cohere`"
            ),
        }
    }

    #[allow(clippy::too_many_arguments)] // endpoint + bounded service-token contract
    pub fn model_plane(
        grpc_url: &str,
        model: &str,
        provider: &str,
        region: &str,
        timeout_ms: u64,
        inference_token_url: &str,
        inference_token_issuer: &str,
        inference_service_id: &str,
        inference_service_api_key: &str,
        inference_retention_posture: &str,
    ) -> anyhow::Result<Self> {
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let retention_posture = RetentionPosture::parse(inference_retention_posture)?;
        let inference_token_client = if standalone_startup() {
            InferenceTokenClient::new_allow_unconfigured(
                inference_token_url,
                inference_token_issuer,
                inference_service_id,
                inference_service_api_key,
                retention_posture,
            )?
        } else {
            InferenceTokenClient::new(
                inference_token_url,
                inference_token_issuer,
                inference_service_id,
                inference_service_api_key,
                retention_posture,
            )?
        };
        let channel = Endpoint::from_shared(grpc_url.to_string())
            .with_context(|| format!("invalid MODEL_PLANE_AI_CORE_GRPC_URL `{grpc_url}`"))?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect_lazy();
        Ok(Self {
            inner: EmbeddingBackend::ModelPlane(Box::new(ModelPlaneEmbeddingClient {
                client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                model: model.to_string(),
                provider: provider.to_string(),
                region: region.trim().to_string(),
                timeout,
                inference_token_client,
            })),
        })
    }

    pub fn azure_openai(endpoint: &str, api_key: &str, deployment: &str) -> anyhow::Result<Self> {
        if endpoint.trim().is_empty() {
            anyhow::bail!("AZURE_OPENAI_ENDPOINT is required when EMBEDDING_PROVIDER=azure_openai");
        }
        if api_key.trim().is_empty() {
            anyhow::bail!("AZURE_OPENAI_API_KEY is required when EMBEDDING_PROVIDER=azure_openai");
        }
        Ok(Self {
            inner: EmbeddingBackend::AzureOpenAi(AzureOpenAiEmbeddingClient {
                http: Client::builder()
                    .timeout(Duration::from_secs(60))
                    .build()
                    .context("build embedding HTTP client")?,
                endpoint: endpoint.trim_end_matches('/').to_string(),
                api_key: api_key.to_string(),
                deployment: deployment.to_string(),
            }),
        })
    }

    pub fn cohere(
        endpoint: &str,
        api_key: &str,
        model: &str,
        api_version: &str,
        output_dimension: u64,
    ) -> anyhow::Result<Self> {
        if endpoint.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_ENDPOINT is required when EMBEDDING_PROVIDER=cohere");
        }
        if api_key.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_API_KEY is required when EMBEDDING_PROVIDER=cohere");
        }
        let output_dimension = validate_matryoshka_dimension(output_dimension, "EMBEDDING_DIMENSION")?;
        Ok(Self {
            inner: EmbeddingBackend::Cohere(CohereEmbeddingClient {
                http: Client::builder()
                    .timeout(Duration::from_secs(60))
                    .build()
                    .context("build Cohere text embedding HTTP client")?,
                endpoint: endpoint.trim_end_matches('/').to_string(),
                api_key: api_key.to_string(),
                model: model.to_string(),
                api_version: api_version.to_string(),
                output_dimension,
            }),
        })
    }

    pub fn provider_name(&self) -> &'static str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(_) => "model_plane",
            EmbeddingBackend::AzureOpenAi(_) => "azure_openai",
            EmbeddingBackend::Cohere(_) => "cohere",
        }
    }

    pub fn model_name(&self) -> &str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => &client.model,
            EmbeddingBackend::AzureOpenAi(client) => &client.deployment,
            EmbeddingBackend::Cohere(client) => &client.model,
        }
    }

    pub async fn embed_batch(
        &self,
        org_id: &str,
        texts: &[String],
        zdr: bool,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => client.embed_batch(org_id, texts, zdr).await,
            EmbeddingBackend::AzureOpenAi(client) => client.embed_batch(texts, zdr).await,
            EmbeddingBackend::Cohere(client) => client.embed_batch(texts, zdr).await,
        }
    }
}

fn standalone_startup() -> bool {
    std::env::var("APP_ENV")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("standalone"))
        .unwrap_or(false)
}

impl ModelPlaneEmbeddingClient {
    async fn embed_batch(
        &self,
        org_id: &str,
        texts: &[String],
        zdr: bool,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let bearer = self.inference_token_client.mint(org_id).await?;
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            vectors.push(self.embed_one(org_id, text, zdr, bearer.as_str()).await?);
        }
        Ok(vectors)
    }

    /// Build the gRPC embedding request. Split out so a unit test can assert the
    /// ZDR signal is faithfully carried onto the wire request without a live
    /// inference-core.
    fn build_request(
        &self,
        org_id: &str,
        text: &str,
        zdr: bool,
    ) -> model_plane::v1::CreateEmbeddingRequest {
        model_plane::v1::CreateEmbeddingRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            text: text.to_string(),
            model: self.model.clone(),
            provider_hint: self.provider.clone(),
            zdr,
            // Forward the requested residency region; inference-core enforces the
            // EU residency gate deny-by-default. Empty = no preference (the EU
            // deployment configured on inference-core is used).
            region: self.region.clone(),
        }
    }

    async fn embed_one(
        &self,
        org_id: &str,
        text: &str,
        zdr: bool,
        bearer: &str,
    ) -> anyhow::Result<Vec<f32>> {
        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                tracing::warn!(attempt, ?backoff, "model-plane embed retry");
                tokio::time::sleep(backoff).await;
            }

            let request = self.build_request(org_id, text, zdr);
            let request = Self::authenticated_request(request, bearer)?;

            let mut client = self.client.clone();
            match tokio::time::timeout(self.timeout, client.create_embedding(request)).await {
                Ok(Ok(resp)) => {
                    let body = resp.into_inner();
                    if body.vector.is_empty() {
                        anyhow::bail!("model-plane embedding returned empty vector");
                    }
                    return Ok(body.vector);
                }
                Ok(Err(status)) if is_retryable_grpc(&status) => {
                    last_err =
                        Some(anyhow::anyhow!(status).context("model-plane embedding failed"));
                }
                Ok(Err(status)) => {
                    return Err(anyhow::anyhow!(status).context("model-plane embedding failed"));
                }
                Err(_) => {
                    last_err = Some(anyhow::anyhow!(
                        "model-plane embedding timed out after {:?}",
                        self.timeout
                    ));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("model-plane embed retries exhausted")))
    }

    fn authenticated_request<T>(message: T, bearer: &str) -> anyhow::Result<tonic::Request<T>> {
        anyhow::ensure!(
            !bearer.is_empty() && !bearer.chars().any(char::is_whitespace),
            "inference bearer is missing or malformed"
        );
        let mut request = tonic::Request::new(message);
        let metadata = MetadataValue::try_from(format!("Bearer {bearer}"))
            .context("inference bearer metadata is invalid")?;
        request.metadata_mut().insert("authorization", metadata);
        Ok(request)
    }
}

impl AzureOpenAiEmbeddingClient {
    async fn embed_batch(&self, texts: &[String], zdr: bool) -> anyhow::Result<Vec<Vec<f32>>> {
        // ZDR egress guard: the direct-Azure path is a *retaining* provider, so
        // documents classified restricted (Zero Data Retention) must never leave
        // on it. Fail closed BEFORE any network call. (An EU/ZDR embedding
        // provider is a Phase-4 prerequisite; until then restricted docs have no
        // compliant embedding path and must error here.)
        if zdr {
            anyhow::bail!("ZDR content must not egress to the direct-Azure embedding path");
        }

        let url = format!(
            "{}/openai/deployments/{}/embeddings?api-version=2024-02-01",
            self.endpoint, self.deployment
        );

        let body = EmbedRequest {
            input: texts.to_vec(),
            model: self.deployment.clone(),
        };

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    INITIAL_BACKOFF_MS * 2u64.pow(attempt),
                ))
                .await;
                tracing::warn!(attempt, "retrying embedding API call");
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
                    let embed_resp: EmbedResponse =
                        resp.json().await.context("parse embedding response")?;
                    return Ok(embed_resp.data.into_iter().map(|d| d.embedding).collect());
                }
                Ok(resp) if resp.status().as_u16() == 429 || resp.status().is_server_error() => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    tracing::warn!(%status, body = %body_text, "embedding API retryable failure");
                    last_err = Some(anyhow::anyhow!("embedding API {status}: {body_text}"));
                    continue;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("embedding API {status}: {body_text}");
                }
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("embedding failed after retries")))
    }
}

impl CohereEmbeddingClient {
    async fn embed_batch(&self, texts: &[String], zdr: bool) -> anyhow::Result<Vec<Vec<f32>>> {
        // ZDR egress guard: Azure Foundry is a *retaining* provider (mirrors the
        // direct-Azure text path and the visual Embed v4 path). Fail closed
        // BEFORE any network call.
        if zdr {
            anyhow::bail!("ZDR content must not egress to the Cohere Embed v4 text path");
        }

        let url = format!(
            "{}/embeddings?api-version={}",
            self.endpoint, self.api_version
        );
        let body = CohereEmbedRequest {
            model: &self.model,
            input: texts,
            input_type: "document",
            output_dimension: self.output_dimension,
        };

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    INITIAL_BACKOFF_MS * 2u64.pow(attempt),
                ))
                .await;
                tracing::warn!(attempt, "retrying Cohere text embedding API call");
            }

            match self
                .http
                .post(&url)
                .header("api-key", &self.api_key)
                // REQUIRED whenever `output_dimension` is on the body. Azure AI
                // Foundry's model-inference gateway validates against the base
                // schema and rejects anything extra with `400: Extra parameters
                // ['output_dimension'] are not allowed when extra-parameters is
                // not set or set to be 'error'`. `pass-through` forwards them.
                //
                // Learned the hard way: a unit test can only assert the field is
                // serialized, so this is invisible until a real call. Without the
                // header EVERY embed 400s — the arm looks configured and embeds
                // nothing.
                .header("extra-parameters", "pass-through")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let embed_resp: EmbedResponse = resp
                        .json()
                        .await
                        .context("parse Cohere text embedding response")?;
                    if embed_resp.data.len() != texts.len() {
                        anyhow::bail!(
                            "Cohere text embedding count mismatch: got {}, want {}",
                            embed_resp.data.len(),
                            texts.len()
                        );
                    }
                    // THE actual guard on the width invariant.
                    //
                    // `main.rs` has already created the Qdrant collection at
                    // `EMBEDDING_DIMENSION`, and Qdrant fixes vector size per
                    // collection. Requesting a width does not guarantee getting
                    // it — Foundry ignores `output_dimension` entirely — so the
                    // only reliable check is on the response. Failing here names
                    // the real cause; without it the mismatch surfaces later as
                    // an opaque Qdrant upsert rejection, or (worse) as an arm
                    // that quietly indexes nothing.
                    //
                    // The query side has always done this
                    // (`retrieval-engine-rs`'s orchestrator validates every
                    // vector it embeds); this closes the same hole on the
                    // indexing side.
                    if let Some(bad) = embed_resp
                        .data
                        .iter()
                        .find(|d| d.embedding.len() as u32 != self.output_dimension)
                    {
                        anyhow::bail!(
                            "Cohere returned {}-dim vectors but EMBEDDING_DIMENSION is {} — the                              Qdrant collection is sized for {} and every upsert would fail. This                              provider ignores `output_dimension`, so set EMBEDDING_DIMENSION to                              the width it actually returns ({}) and re-embed into a fresh                              collection.",
                            bad.embedding.len(),
                            self.output_dimension,
                            self.output_dimension,
                            bad.embedding.len()
                        );
                    }
                    return Ok(embed_resp.data.into_iter().map(|d| d.embedding).collect());
                }
                Ok(resp) if resp.status().as_u16() == 429 || resp.status().is_server_error() => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    tracing::warn!(%status, body = %body_text, "Cohere text embedding retryable failure");
                    last_err = Some(anyhow::anyhow!(
                        "Cohere text embedding API {status}: {body_text}"
                    ));
                    continue;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("Cohere text embedding API {status}: {body_text}");
                }
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("Cohere text embedding failed after retries")))
    }
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "model-plane" | "modelplane" | "inference-core" | "inference_core" | "ai-core"
        | "ai_core" => "model_plane".to_string(),
        "azure" | "azure-openai" => "azure_openai".to_string(),
        "cohere-embed-v4" | "cohere_embed_v4" | "embed-v4" | "embed_v4" => "cohere".to_string(),
        other => other.to_string(),
    }
}

fn is_retryable_grpc(status: &tonic::Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::DeadlineExceeded
            | tonic::Code::Internal
            | tonic::Code::ResourceExhausted
            | tonic::Code::Unavailable
            | tonic::Code::Unknown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_provider_aliases() {
        assert_eq!(normalize_provider("model-plane"), "model_plane");
        assert_eq!(normalize_provider("inference_core"), "model_plane");
        assert_eq!(normalize_provider("ai_core"), "model_plane");
        assert_eq!(normalize_provider("azure-openai"), "azure_openai");
        assert_eq!(normalize_provider("cohere-embed-v4"), "cohere");
        assert_eq!(normalize_provider("embed_v4"), "cohere");
        assert_eq!(normalize_provider("cohere"), "cohere");
    }

    #[test]
    fn cohere_requires_endpoint_and_key() {
        let err = match EmbeddingProvider::cohere("", "", "Cohere-embed-4", "2024-05-01-preview", 1536) {
            Ok(_) => panic!("empty endpoint should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("COHERE_EMBED_V4_ENDPOINT"),
            "unexpected error: {err}"
        );

        let err = match EmbeddingProvider::cohere(
            "https://x.services.ai.azure.com",
            "",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        ) {
            Ok(_) => panic!("empty api key should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("COHERE_EMBED_V4_API_KEY"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn cohere_provider_selects_cohere_backend() {
        let provider = EmbeddingProvider::cohere(
            "https://x.services.ai.azure.com",
            "k",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere provider");
        assert_eq!(provider.provider_name(), "cohere");
        assert_eq!(provider.model_name(), "Cohere-embed-4");
    }

    /// Pins the Foundry text-embeddings request shape: plain string `input`
    /// (not the visual arm's `{image, text}` objects) and `input_type:
    /// "document"` for the indexing side.
    #[test]
    fn cohere_request_serializes_to_foundry_text_embeddings_shape() {
        let texts = vec!["hei".to_string(), "verden".to_string()];
        let req = CohereEmbedRequest {
            model: "Cohere-embed-4",
            input: &texts,
            input_type: "document",
            output_dimension: 1536,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "Cohere-embed-4");
        assert_eq!(v["input_type"], "document");
        assert_eq!(v["input"][0], "hei");
        assert_eq!(v["input"][1], "verden");
        assert_eq!(
            v["output_dimension"], 1536,
            "the Matryoshka width must be on the wire, not left to the API default"
        );
    }

    /// The trap this closes: `EMBEDDING_DIMENSION` sizes the Qdrant collection,
    /// so a width that never reached Cohere produced a collection and a vector
    /// that disagreed — a write-time failure for whoever tuned it down.
    #[test]
    fn a_truncated_matryoshka_width_reaches_the_wire() {
        let texts = vec!["hei".to_string()];
        for dim in COHERE_MATRYOSHKA_DIMENSIONS {
            let width = validate_matryoshka_dimension(dim, "EMBEDDING_DIMENSION")
                .expect("supported Matryoshka width");
            let req = CohereEmbedRequest {
                model: "Cohere-embed-4",
                input: &texts,
                input_type: "document",
                output_dimension: width,
            };
            let v = serde_json::to_value(&req).unwrap();
            assert_eq!(v["output_dimension"], dim);
        }
    }

    /// Fail closed at construction, not on first embed: the bad-width service
    /// would otherwise boot healthy and create a Qdrant collection Cohere can
    /// never fill.
    #[test]
    fn an_unsupported_dimension_is_rejected_before_any_collection_is_created() {
        for bad in [0_u64, 1, 384, 768, 1024 + 1, 3072] {
            let err = match EmbeddingProvider::cohere(
                "https://x.services.ai.azure.com",
                "k",
                "Cohere-embed-4",
                "2024-05-01-preview",
                bad,
            ) {
                Ok(_) => panic!("width {bad} must fail closed"),
                Err(err) => err,
            };
            assert!(
                err.to_string().contains("Matryoshka"),
                "unexpected error for {bad}: {err}"
            );
        }
        // 768 is a plausible-looking mistake (it is the *video* arm's width),
        // so confirm the supported set still builds.
        for good in COHERE_MATRYOSHKA_DIMENSIONS {
            if let Err(e) = EmbeddingProvider::cohere(
                "https://x.services.ai.azure.com",
                "k",
                "Cohere-embed-4",
                "2024-05-01-preview",
                good,
            ) {
                panic!("{good} must be accepted: {e}");
            }
        }
    }

    /// ZDR egress guard: a restricted chunk must fail closed BEFORE any
    /// network call (the unreachable endpoint would surface a connection
    /// error if the guard regressed, not the ZDR error asserted here).
    #[tokio::test]
    async fn cohere_egress_guard_rejects_zdr() {
        let provider = EmbeddingProvider::cohere(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere provider");
        let err = provider
            .embed_batch("org-1", &["restricted doc chunk".to_string()], true)
            .await
            .expect_err("restricted (ZDR) content must not egress to Cohere Embed v4");
        assert!(
            err.to_string()
                .contains("must not egress to the Cohere Embed v4 text path"),
            "unexpected error: {err}"
        );
    }

    /// Non-restricted docs still embed (guard trips only for ZDR=true); the
    /// unreachable endpoint yields a network error, NOT the egress error.
    #[tokio::test]
    async fn cohere_allows_non_zdr() {
        let provider = EmbeddingProvider::cohere(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere provider");
        let err = provider
            .embed_batch("org-1", &["public doc chunk".to_string()], false)
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the Cohere Embed v4 text path"),
            "non-ZDR content must not hit the egress guard: {err}"
        );
    }

    #[test]
    fn model_plane_requires_scoped_service_token_configuration() {
        let error = match EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            "swedencentral",
            30_000,
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "",
            "persistent",
        ) {
            Ok(_) => panic!("embedding must fail closed without a service credential"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("MODEL_PLANE_INFERENCE_SERVICE_API_KEY"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn model_plane_provider_selects_model_plane_backend() {
        let provider = EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            "swedencentral",
            30_000,
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "service-key-for-embedding-tests",
            "persistent",
        )
        .expect("model-plane provider");
        assert_eq!(provider.provider_name(), "model_plane");
        assert_eq!(provider.model_name(), "text-embedding-3-large");
    }

    #[test]
    fn azure_openai_requires_direct_credentials() {
        let err = match EmbeddingProvider::azure_openai("", "", "text-embedding-3-large") {
            Ok(_) => panic!("empty direct Azure config should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("AZURE_OPENAI_ENDPOINT"),
            "unexpected error: {err}"
        );
    }

    /// ZDR egress guard on the doc-embedding path: a restricted document must
    /// fail closed on the direct-Azure provider BEFORE any network call (Azure
    /// is a retaining provider). The unreachable endpoint guarantees a regressed
    /// guard would surface a connection error, not the ZDR error asserted here.
    #[tokio::test]
    async fn azure_openai_egress_guard_rejects_zdr() {
        let provider = EmbeddingProvider::azure_openai(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "text-embedding-3-large",
        )
        .expect("direct Azure provider");
        let err = provider
            .embed_batch("org-1", &["restricted doc chunk".to_string()], true)
            .await
            .expect_err("restricted (ZDR) content must not egress to direct-Azure");
        assert!(
            err.to_string()
                .contains("must not egress to the direct-Azure embedding path"),
            "unexpected error: {err}"
        );
    }

    /// Non-restricted docs still embed on the direct-Azure path (guard trips
    /// only for ZDR=true). The unreachable endpoint yields a network error, NOT
    /// the egress error — proving the guard is not constant-on.
    #[tokio::test]
    async fn azure_openai_allows_non_zdr() {
        let provider = EmbeddingProvider::azure_openai(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "text-embedding-3-large",
        )
        .expect("direct Azure provider");
        let err = provider
            .embed_batch("org-1", &["public doc chunk".to_string()], false)
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the direct-Azure embedding path"),
            "non-ZDR content must not hit the egress guard: {err}"
        );
    }

    /// ModelPlane path: the ZDR signal and the residency region are faithfully
    /// placed on the wire request (both true and false ZDR round-trip from the
    /// caller's argument; the configured region is forwarded verbatim).
    #[tokio::test]
    async fn model_plane_request_carries_zdr_and_region() {
        let provider = EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            "swedencentral",
            30_000,
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "service-key-for-embedding-tests",
            "persistent",
        )
        .expect("model-plane provider");
        let EmbeddingBackend::ModelPlane(inner) = &provider.inner else {
            panic!("expected a model-plane backend");
        };
        assert!(
            inner.build_request("org-1", "hi", true).zdr,
            "zdr=true must propagate"
        );
        assert!(
            !inner.build_request("org-1", "hi", false).zdr,
            "zdr=false must propagate"
        );
        assert_eq!(
            inner.build_request("org-1", "hi", false).region,
            "swedencentral",
            "residency region must propagate onto the wire request"
        );
    }

    #[tokio::test]
    async fn model_plane_request_requires_and_sets_bearer_auth() {
        let provider = EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            "swedencentral",
            30_000,
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "service-key-for-embedding-tests",
            "persistent",
        )
        .expect("model-plane provider");
        let EmbeddingBackend::ModelPlane(inner) = &provider.inner else {
            panic!("expected a model-plane backend");
        };

        let request = ModelPlaneEmbeddingClient::authenticated_request(
            inner.build_request("org-1", "hi", false),
            "verified-inference-token",
        )
        .expect("verified bearer should be accepted");
        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer verified-inference-token")
        );

        let error = ModelPlaneEmbeddingClient::authenticated_request(
            inner.build_request("org-1", "hi", false),
            "",
        )
        .expect_err("missing bearer must fail closed");
        assert!(error.to_string().contains("inference bearer"));
    }
}
