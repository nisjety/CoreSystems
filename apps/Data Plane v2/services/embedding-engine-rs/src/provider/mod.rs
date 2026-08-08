use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;

mod inference_auth;
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
    ) -> anyhow::Result<Self> {
        if endpoint.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_ENDPOINT is required when EMBEDDING_PROVIDER=cohere");
        }
        if api_key.trim().is_empty() {
            anyhow::bail!("COHERE_EMBED_V4_API_KEY is required when EMBEDDING_PROVIDER=cohere");
        }
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
        let err = match EmbeddingProvider::cohere("", "", "Cohere-embed-4", "2024-05-01-preview") {
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
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "Cohere-embed-4");
        assert_eq!(v["input_type"], "document");
        assert_eq!(v["input"][0], "hei");
        assert_eq!(v["input"][1], "verden");
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
