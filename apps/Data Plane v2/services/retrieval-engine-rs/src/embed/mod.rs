use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;

pub mod media;
mod service_auth;
pub mod visual;

use service_auth::{InferenceBearer, InferenceTokenClient, RetentionPosture};

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 200;

mod model_plane {
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

#[derive(Clone)]
pub struct EmbeddingClient {
    inner: EmbeddingBackend,
}

#[derive(Clone)]
enum EmbeddingBackend {
    ModelPlane(Box<ModelPlaneEmbeddingClient>),
    AzureOpenAi(AzureOpenAiEmbeddingClient),
    Cohere(CohereEmbeddingClient),
    DeterministicTest { dimension: usize },
}

#[derive(Clone)]
struct ModelPlaneEmbeddingClient {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    model: String,
    provider: String,
    timeout: Duration,
    token_client: InferenceTokenClient,
}

struct ModelPlaneSettings<'a> {
    grpc_url: &'a str,
    model: &'a str,
    provider: &'a str,
    timeout_ms: u64,
    token_url: &'a str,
    token_issuer: &'a str,
    service_id: &'a str,
    service_api_key: &'a str,
    retention_posture: &'a str,
}

#[derive(Clone)]
struct AzureOpenAiEmbeddingClient {
    http: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
}

/// Cohere Embed v4 (Azure AI Foundry) — dense **text** query embeddings.
/// Mirrors `embedding-engine-rs/src/provider/mod.rs`'s `CohereEmbeddingClient`
/// exactly, with `input_type: "query"` instead of `"document"` — Embed v4
/// asymmetrically optimizes each side for retrieval. Shares the deployment
/// the visual arm already uses (`embed/visual.rs`).
#[derive(Clone)]
struct CohereEmbeddingClient {
    http: Client,
    endpoint: String,
    api_key: String,
    model: String,
    api_version: String,
    /// Matryoshka output width. See [`validate_matryoshka_dimension`] — this
    /// MUST equal the indexing side's `EMBEDDING_DIMENSION`, or query vectors
    /// and stored vectors have different widths and Qdrant rejects the search.
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
    /// Matryoshka truncation width, always explicit — see the indexing-side
    /// twin in `embedding-engine-rs/src/provider/mod.rs` for why omitting it
    /// silently returns the native 1536 no matter what the config says.
    output_dimension: u32,
}

/// Cohere Embed v4's Matryoshka-supported output widths — the query-side twin
/// of `embedding-engine-rs`'s identical constant. Duplicated rather than shared
/// because these two services deploy independently: a shared crate would let a
/// single edit silently move both sides at once, and the whole point of
/// validating here is that the query side fails on its own if it is configured
/// to a width the corpus was not indexed at.
const COHERE_MATRYOSHKA_DIMENSIONS: [usize; 4] = [256, 512, 1024, 1536];

/// Fail closed at startup on a width Embed v4 cannot produce.
///
/// The query side has a second, sharper reason to validate than the indexing
/// side: a mismatched query width does not fail at write time, it fails on
/// every single search. Catching it at construction turns a total retrieval
/// outage into a refused boot.
fn validate_matryoshka_dimension(dimension: usize, env_var: &str) -> anyhow::Result<u32> {
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

impl EmbeddingClient {
    pub fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        match normalize_provider(&cfg.embedding_provider).as_str() {
            "model_plane" => Self::model_plane(ModelPlaneSettings {
                grpc_url: &cfg.model_plane_ai_core_grpc_url,
                model: &cfg.azure_openai_embedding_deployment,
                provider: &cfg.model_plane_embedding_provider,
                timeout_ms: cfg.model_plane_embedding_timeout_ms,
                token_url: &cfg.model_plane_inference_token_url,
                token_issuer: &cfg.model_plane_inference_token_issuer,
                service_id: &cfg.model_plane_inference_service_id,
                service_api_key: &cfg.model_plane_inference_service_api_key,
                retention_posture: &cfg.model_plane_inference_retention_posture,
            }),
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
                // Same field the orchestrator already length-checks every
                // returned vector against (`config.embedding_dimension`), so
                // the wire width and the assertion width are one value.
                cfg.embedding_dimension,
            ),
            "deterministic_test"
                if deterministic_test_allowed(
                    std::env::var("ALLOW_INSECURE_DEV_DEFAULTS").ok().as_deref(),
                    std::env::var("ISOLATED_E2E").ok().as_deref(),
                ) =>
            {
                Self::deterministic_test(cfg.embedding_dimension)
            }
            other => anyhow::bail!(
                "unsupported EMBEDDING_PROVIDER `{other}`; expected `model_plane`, `azure_openai`, or `cohere`"
            ),
        }
    }

    fn model_plane(settings: ModelPlaneSettings<'_>) -> anyhow::Result<Self> {
        let timeout = Duration::from_millis(settings.timeout_ms.max(1));
        let channel = Endpoint::from_shared(settings.grpc_url.to_string())
            .with_context(|| {
                format!(
                    "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                    settings.grpc_url
                )
            })?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect_lazy();
        let token_client = InferenceTokenClient::new(
            settings.token_url,
            settings.token_issuer,
            settings.service_id,
            settings.service_api_key,
            RetentionPosture::parse(settings.retention_posture)?,
        )?;
        Ok(Self {
            inner: EmbeddingBackend::ModelPlane(Box::new(ModelPlaneEmbeddingClient {
                client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                model: settings.model.to_string(),
                provider: settings.provider.to_string(),
                timeout,
                token_client,
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
                http: Client::new(),
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
        output_dimension: usize,
    ) -> anyhow::Result<Self> {
        if endpoint.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_ENDPOINT is required when EMBEDDING_PROVIDER=cohere");
        }
        if api_key.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_API_KEY is required when EMBEDDING_PROVIDER=cohere");
        }
        let output_dimension =
            validate_matryoshka_dimension(output_dimension, "EMBEDDING_DIMENSION")?;
        Ok(Self {
            inner: EmbeddingBackend::Cohere(CohereEmbeddingClient {
                http: Client::new(),
                endpoint: endpoint.trim_end_matches('/').to_string(),
                api_key: api_key.to_string(),
                model: model.to_string(),
                api_version: api_version.to_string(),
                output_dimension,
            }),
        })
    }

    fn deterministic_test(dimension: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=65_536).contains(&dimension),
            "deterministic test embedding dimension is invalid"
        );
        Ok(Self {
            inner: EmbeddingBackend::DeterministicTest { dimension },
        })
    }

    pub fn provider_name(&self) -> &'static str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(_) => "model_plane",
            EmbeddingBackend::AzureOpenAi(_) => "azure_openai",
            EmbeddingBackend::Cohere(_) => "cohere",
            EmbeddingBackend::DeterministicTest { .. } => "deterministic_test",
        }
    }

    pub fn model_name(&self) -> &str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => &client.model,
            EmbeddingBackend::AzureOpenAi(client) => &client.deployment,
            EmbeddingBackend::Cohere(client) => &client.model,
            EmbeddingBackend::DeterministicTest { .. } => "deterministic-isolated",
        }
    }

    pub fn cache_namespace(&self) -> String {
        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => {
                format!(
                    "{}:{}:{}",
                    self.provider_name(),
                    client.provider,
                    client.model
                )
            }
            EmbeddingBackend::AzureOpenAi(client) => {
                format!("{}:{}", self.provider_name(), client.deployment)
            }
            EmbeddingBackend::Cohere(client) => {
                format!("{}:{}", self.provider_name(), client.model)
            }
            EmbeddingBackend::DeterministicTest { dimension } => {
                format!("deterministic_test:{dimension}")
            }
        }
    }

    #[tracing::instrument(
        name = "embedding.query",
        skip(self, text),
        fields(otel.kind = "client", org_id = %org_id, text_len = text.len(), zdr = zdr),
    )]
    pub async fn embed_query(
        &self,
        org_id: &str,
        text: &str,
        zdr: bool,
    ) -> anyhow::Result<Vec<f32>> {
        let vectors = self.embed_batch(org_id, &[text.to_string()], zdr).await?;
        vectors
            .into_iter()
            .next()
            .context("empty embedding response")
    }

    #[tracing::instrument(
        name = "embedding.batch",
        skip(self, texts),
        fields(otel.kind = "client", org_id = %org_id, batch_size = texts.len(), zdr = zdr),
    )]
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
            EmbeddingBackend::DeterministicTest { dimension } => Ok(texts
                .iter()
                .map(|text| deterministic_vector(org_id, text, *dimension))
                .collect()),
        }
    }
}

fn deterministic_test_allowed(
    allow_insecure_dev_defaults: Option<&str>,
    isolated_e2e: Option<&str>,
) -> bool {
    allow_insecure_dev_defaults == Some("1") && isolated_e2e == Some("1")
}

fn deterministic_vector(org_id: &str, text: &str, dimension: usize) -> Vec<f32> {
    let digest = blake3::hash(format!("{org_id}\0{text}").as_bytes());
    let bytes = digest.as_bytes();
    (0..dimension)
        .map(|index| (f32::from(bytes[index % bytes.len()]) / 127.5) - 1.0)
        .collect()
}

impl ModelPlaneEmbeddingClient {
    async fn embed_batch(
        &self,
        org_id: &str,
        texts: &[String],
        zdr: bool,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        // Mint once per organization-scoped batch. The signed token is bounded
        // to this tenant, exact inference scope, caller identity, short TTL and
        // issuer-enforced ZDR. A mint/validation failure stops before gRPC.
        let bearer = self
            .token_client
            .mint(org_id)
            .await
            .context("mint bounded inference credential")?;
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            vectors.push(self.embed_one(&bearer, org_id, text, zdr).await?);
        }
        Ok(vectors)
    }

    /// Build the gRPC embedding request. Split out so a unit test can assert
    /// the ZDR signal is faithfully carried onto the wire request without a
    /// live inference-core.
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
            // Empty = no caller preference → inference-core uses its configured
            // EU deployment (deny-by-default on non-EU). Query embeddings ride the
            // same EU default; a specific region can be wired here later.
            region: String::new(),
        }
    }

    fn build_authenticated_request(
        &self,
        bearer: &str,
        org_id: &str,
        text: &str,
        zdr: bool,
    ) -> anyhow::Result<tonic::Request<model_plane::v1::CreateEmbeddingRequest>> {
        let mut request = tonic::Request::new(self.build_request(org_id, text, zdr));
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {bearer}"))
                .context("bounded inference bearer is not valid gRPC metadata")?,
        );
        Ok(request)
    }

    async fn embed_one(
        &self,
        bearer: &InferenceBearer,
        org_id: &str,
        text: &str,
        zdr: bool,
    ) -> anyhow::Result<Vec<f32>> {
        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                tracing::warn!(attempt, ?backoff, "model-plane embed retry");
                tokio::time::sleep(backoff).await;
            }

            let request = self.build_authenticated_request(bearer.as_str(), org_id, text, zdr)?;

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
}

impl AzureOpenAiEmbeddingClient {
    async fn embed_batch(&self, texts: &[String], zdr: bool) -> anyhow::Result<Vec<Vec<f32>>> {
        // ZDR egress guard: the direct-Azure path is a *retaining* provider, so
        // Zero-Data-Retention content must never leave on it. Fail closed BEFORE
        // any network call rather than egress and hope. (An EU/ZDR embedding
        // provider is a Phase-4 prerequisite; until then ZDR content has no
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
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                tracing::warn!(attempt, ?backoff, "embed retry");
                tokio::time::sleep(backoff).await;
            }

            let resp = match self
                .http
                .post(&url)
                .header("api-key", &self.api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(anyhow::anyhow!(e).context("embedding API call failed"));
                    continue;
                }
            };

            if resp.status().is_server_error() || resp.status().as_u16() == 429 {
                let status = resp.status();
                last_err = Some(sanitized_provider_status_error("embedding API", status));
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                return Err(sanitized_provider_status_error("embedding API", status));
            }

            let embed_resp: EmbedResponse =
                resp.json().await.context("parse embedding response")?;
            return Ok(embed_resp.data.into_iter().map(|d| d.embedding).collect());
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("embed retries exhausted")))
    }
}

impl CohereEmbeddingClient {
    async fn embed_batch(&self, texts: &[String], zdr: bool) -> anyhow::Result<Vec<Vec<f32>>> {
        // ZDR egress guard: Azure Foundry is a *retaining* provider (mirrors
        // the direct-Azure path and embedding-engine-rs's indexing-side twin).
        if zdr {
            anyhow::bail!("ZDR content must not egress to the Cohere Embed v4 text path");
        }

        let url = format!(
            "{}/embeddings?api-version={}",
            self.endpoint, self.api_version
        );
        let body = CohereEmbedRequest {
            output_dimension: self.output_dimension,
            model: &self.model,
            input: texts,
            // Query side: Embed v4 asymmetrically optimizes query vs document
            // embeddings for retrieval. The indexing side
            // (embedding-engine-rs) sends "document".
            input_type: "query",
        };

        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                tracing::warn!(attempt, ?backoff, "Cohere text embed retry");
                tokio::time::sleep(backoff).await;
            }

            let resp = match self
                .http
                .post(&url)
                .header("api-key", &self.api_key)
                // REQUIRED whenever `output_dimension` is on the body. Azure AI
                // Foundry's model-inference gateway validates against the base
                // schema and rejects anything extra with `400: Extra parameters
                // ['output_dimension'] are not allowed when extra-parameters is
                // not set or set to be 'error'`. `pass-through` forwards them.
                //
                // Learned the hard way on the indexing side: a unit test can
                // only assert the field is serialized, so this is invisible
                // until a real call. Without the header EVERY embed 400s — here
                // that means every query fails, not just every write.
                .header("extra-parameters", "pass-through")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err =
                        Some(anyhow::anyhow!(e).context("Cohere text embedding API call failed"));
                    continue;
                }
            };

            if resp.status().is_server_error() || resp.status().as_u16() == 429 {
                let status = resp.status();
                last_err = Some(sanitized_provider_status_error(
                    "Cohere text embedding API",
                    status,
                ));
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status();
                return Err(sanitized_provider_status_error(
                    "Cohere text embedding API",
                    status,
                ));
            }

            let embed_resp: EmbedResponse = resp
                .json()
                .await
                .context("parse Cohere text embedding response")?;
            return Ok(embed_resp.data.into_iter().map(|d| d.embedding).collect());
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Cohere text embed retries exhausted")))
    }
}

fn sanitized_provider_status_error(provider: &str, status: reqwest::StatusCode) -> anyhow::Error {
    anyhow::anyhow!("{provider} returned HTTP status {}", status.as_u16())
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
        let err =
            match EmbeddingClient::cohere("", "", "Cohere-embed-4", "2024-05-01-preview", 1536) {
                Ok(_) => panic!("empty endpoint should fail"),
                Err(err) => err,
            };
        assert!(
            err.to_string().contains("COHERE_EMBED_V4_ENDPOINT"),
            "unexpected error: {err}"
        );

        let err = match EmbeddingClient::cohere(
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
    fn cohere_client_names_cache_by_provider_and_model() {
        let client = EmbeddingClient::cohere(
            "https://x.services.ai.azure.com",
            "k",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere client");
        assert_eq!(client.provider_name(), "cohere");
        assert_eq!(client.model_name(), "Cohere-embed-4");
        assert_eq!(client.cache_namespace(), "cohere:Cohere-embed-4");
    }

    /// Pins the query-side request shape: plain string `input` and
    /// `input_type: "query"` — the asymmetric counterpart to
    /// embedding-engine-rs's indexing-side `"document"`.
    #[test]
    fn cohere_request_serializes_to_foundry_query_embeddings_shape() {
        let texts = vec!["hva er prisen?".to_string()];
        let req = CohereEmbedRequest {
            model: "Cohere-embed-4",
            input: &texts,
            input_type: "query",
            output_dimension: 1536,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "Cohere-embed-4");
        assert_eq!(v["input_type"], "query");
        assert_eq!(v["input"][0], "hva er prisen?");
        assert_eq!(
            v["output_dimension"], 1536,
            "the query width must be explicit or it silently reverts to native 1536"
        );
    }

    /// A query embedded at a different Matryoshka width than the corpus was
    /// indexed at fails on *every* search, not at write time — so the width
    /// must be refused at boot rather than discovered in production.
    #[test]
    fn an_unsupported_query_dimension_refuses_to_construct() {
        for bad in [0_usize, 1, 384, 768, 3072] {
            let err = match EmbeddingClient::cohere(
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
        // Every truncation width Embed v4 actually supports must still build,
        // and must carry that exact width onto the wire.
        for good in COHERE_MATRYOSHKA_DIMENSIONS {
            let width = validate_matryoshka_dimension(good, "EMBEDDING_DIMENSION")
                .unwrap_or_else(|e| panic!("{good} must be accepted: {e}"));
            assert_eq!(width as usize, good);
            if let Err(e) = EmbeddingClient::cohere(
                "https://x.services.ai.azure.com",
                "k",
                "Cohere-embed-4",
                "2024-05-01-preview",
                good,
            ) {
                panic!("{good} must build: {e}");
            }
        }
    }

    /// The two engines are deployed separately, so their width lists must stay
    /// literally identical — a value one side accepts and the other rejects is
    /// a half-adopted dimension change, which is the failure this guards.
    #[test]
    fn the_supported_width_set_matches_the_indexing_side() {
        assert_eq!(
            COHERE_MATRYOSHKA_DIMENSIONS,
            [256, 512, 1024, 1536],
            "keep in lockstep with embedding-engine-rs::provider::COHERE_MATRYOSHKA_DIMENSIONS"
        );
    }

    /// ZDR egress guard: a restricted query must fail closed BEFORE any
    /// network call (the unreachable endpoint would surface a connection
    /// error if the guard regressed, not the ZDR error asserted here).
    #[tokio::test]
    async fn cohere_egress_guard_rejects_zdr() {
        let client = EmbeddingClient::cohere(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere client");
        let err = client
            .embed_batch("org-1", &["restricted query".to_string()], true)
            .await
            .expect_err("ZDR content must not egress to Cohere Embed v4");
        assert!(
            err.to_string()
                .contains("must not egress to the Cohere Embed v4 text path"),
            "unexpected error: {err}"
        );
    }

    /// Non-ZDR queries still embed (the guard only trips for ZDR=true); the
    /// unreachable endpoint yields a network/retry error, NOT the egress
    /// error — proving the guard is not constant-on.
    #[tokio::test]
    async fn cohere_allows_non_zdr() {
        let client = EmbeddingClient::cohere(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "Cohere-embed-4",
            "2024-05-01-preview",
            1536,
        )
        .expect("cohere client");
        let err = client
            .embed_batch("org-1", &["public query".to_string()], false)
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the Cohere Embed v4 text path"),
            "non-ZDR content must not hit the egress guard: {err}"
        );
    }

    #[test]
    fn upstream_status_errors_never_include_provider_response_bodies() {
        let error = sanitized_provider_status_error(
            "embedding API",
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .to_string();
        assert_eq!(error, "embedding API returned HTTP status 500");
        assert!(!error.contains("response"));
    }

    #[test]
    fn deterministic_backend_requires_two_explicit_isolation_gates() {
        assert!(!deterministic_test_allowed(Some("1"), None));
        assert!(!deterministic_test_allowed(None, Some("1")));
        assert!(!deterministic_test_allowed(Some("true"), Some("1")));
        assert!(deterministic_test_allowed(Some("1"), Some("1")));
    }

    #[tokio::test]
    async fn deterministic_backend_is_stable_and_dimension_bounded() {
        let client = EmbeddingClient::deterministic_test(8).expect("test backend");
        let first = client
            .embed_query("org-1", "isolated query", true)
            .await
            .expect("embedding");
        let second = client
            .embed_query("org-1", "isolated query", true)
            .await
            .expect("embedding");
        assert_eq!(first, second);
        assert_eq!(first.len(), 8);
    }

    #[tokio::test]
    async fn model_plane_client_names_cache_by_plane_provider_and_model() {
        let client = EmbeddingClient::model_plane(ModelPlaneSettings {
            grpc_url: "http://inference-core:9092",
            model: "text-embedding-3-large",
            provider: "azure_openai",
            timeout_ms: 30_000,
            token_url: "http://auth-core:3011/api/inference-core/internal-token",
            token_issuer: "http://localhost:3011/api/convex-auth",
            service_id: "retrieval-engine",
            service_api_key: "isolated-test-service-credential",
            retention_posture: "persistent",
        })
        .expect("model-plane client");
        assert_eq!(client.provider_name(), "model_plane");
        assert_eq!(
            client.cache_namespace(),
            "model_plane:azure_openai:text-embedding-3-large"
        );
    }

    #[test]
    fn azure_openai_requires_direct_credentials() {
        let err = match EmbeddingClient::azure_openai("", "", "text-embedding-3-large") {
            Ok(_) => panic!("empty direct Azure config should fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("AZURE_OPENAI_ENDPOINT"),
            "unexpected error: {err}"
        );
    }

    /// ZDR egress guard: the direct-Azure embedding path must fail closed for
    /// ZDR content and must do so BEFORE any network call (Azure is a retaining
    /// provider). The fake endpoint guarantees that if the guard regressed we'd
    /// get a connection error, not the ZDR error asserted here.
    #[tokio::test]
    async fn azure_openai_egress_guard_rejects_zdr() {
        let client = EmbeddingClient::azure_openai(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "text-embedding-3-large",
        )
        .expect("direct Azure client");
        let err = client
            .embed_batch("org-1", &["restricted text".to_string()], true)
            .await
            .expect_err("ZDR content must not egress to direct-Azure");
        assert!(
            err.to_string()
                .contains("must not egress to the direct-Azure embedding path"),
            "unexpected error: {err}"
        );
    }

    /// Non-ZDR content still embeds on the direct-Azure path (the guard only
    /// trips for ZDR=true). Here the unreachable endpoint means we expect a
    /// network/retry error, NOT the egress-guard error — proving the guard is
    /// not constant-on.
    #[tokio::test]
    async fn azure_openai_allows_non_zdr() {
        let client = EmbeddingClient::azure_openai(
            "http://127.0.0.1:1/unreachable",
            "fake-key",
            "text-embedding-3-large",
        )
        .expect("direct Azure client");
        let err = client
            .embed_batch("org-1", &["public text".to_string()], false)
            .await
            .expect_err("unreachable endpoint should error");
        assert!(
            !err.to_string()
                .contains("must not egress to the direct-Azure embedding path"),
            "non-ZDR content must not hit the egress guard: {err}"
        );
    }

    /// ModelPlane path: the ZDR signal is faithfully placed on the wire request
    /// (true and false both round-trip from the caller's argument).
    #[tokio::test]
    async fn model_plane_request_carries_zdr() {
        let client = EmbeddingClient::model_plane(ModelPlaneSettings {
            grpc_url: "http://inference-core:9092",
            model: "text-embedding-3-large",
            provider: "azure_openai",
            timeout_ms: 30_000,
            token_url: "http://auth-core:3011/api/inference-core/internal-token",
            token_issuer: "http://localhost:3011/api/convex-auth",
            service_id: "retrieval-engine",
            service_api_key: "isolated-test-service-credential",
            retention_posture: "persistent",
        })
        .expect("model-plane client");
        let EmbeddingBackend::ModelPlane(inner) = &client.inner else {
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
    }

    #[tokio::test]
    async fn model_plane_request_forwards_only_bounded_bearer_and_zdr() {
        let client = EmbeddingClient::model_plane(ModelPlaneSettings {
            grpc_url: "http://inference-core:9092",
            model: "text-embedding-3-large",
            provider: "azure_openai",
            timeout_ms: 30_000,
            token_url: "http://auth-core:3011/api/inference-core/internal-token",
            token_issuer: "http://localhost:3011/api/convex-auth",
            service_id: "retrieval-engine",
            service_api_key: "isolated-test-service-credential",
            retention_posture: "persistent",
        })
        .expect("model-plane client");
        let EmbeddingBackend::ModelPlane(inner) = &client.inner else {
            panic!("expected a model-plane backend");
        };

        let request = inner
            .build_authenticated_request(
                "header.payload.signature",
                "org-1",
                "restricted query",
                true,
            )
            .expect("authenticated request");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer header.payload.signature")
        );
        assert!(request.metadata().get("x-api-key").is_none());
        assert!(request.get_ref().zdr, "zdr=true must survive auth wrapping");
        assert_eq!(request.get_ref().org_id, "org-1");
    }
}
