use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;

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
    ModelPlane(ModelPlaneEmbeddingClient),
    AzureOpenAi(AzureOpenAiEmbeddingClient),
}

#[derive(Clone)]
struct ModelPlaneEmbeddingClient {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    model: String,
    provider: String,
    timeout: Duration,
    internal_api_key: Option<String>,
}

#[derive(Clone)]
struct AzureOpenAiEmbeddingClient {
    http: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
}

#[derive(Serialize)]
struct EmbedRequest {
    input: Vec<String>,
    model: String,
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
                cfg.model_plane_embedding_timeout_ms,
                cfg.internal_api_key.clone(),
            ),
            "azure_openai" => Self::azure_openai(
                &cfg.azure_openai_endpoint,
                &cfg.azure_openai_api_key,
                &cfg.azure_openai_embedding_deployment,
            ),
            other => anyhow::bail!(
                "unsupported EMBEDDING_PROVIDER `{other}`; expected `model_plane` or `azure_openai`"
            ),
        }
    }

    pub fn model_plane(
        grpc_url: &str,
        model: &str,
        provider: &str,
        timeout_ms: u64,
        internal_api_key: Option<String>,
    ) -> anyhow::Result<Self> {
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let channel = Endpoint::from_shared(grpc_url.to_string())
            .with_context(|| format!("invalid MODEL_PLANE_AI_CORE_GRPC_URL `{grpc_url}`"))?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect_lazy();
        Ok(Self {
            inner: EmbeddingBackend::ModelPlane(ModelPlaneEmbeddingClient {
                client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                model: model.to_string(),
                provider: provider.to_string(),
                timeout,
                internal_api_key: internal_api_key.filter(|key| !key.is_empty()),
            }),
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

    pub fn provider_name(&self) -> &'static str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(_) => "model_plane",
            EmbeddingBackend::AzureOpenAi(_) => "azure_openai",
        }
    }

    pub fn model_name(&self) -> &str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => &client.model,
            EmbeddingBackend::AzureOpenAi(client) => &client.deployment,
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
        }
    }
}

impl ModelPlaneEmbeddingClient {
    async fn embed_batch(
        &self,
        org_id: &str,
        texts: &[String],
        zdr: bool,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            vectors.push(self.embed_one(org_id, text, zdr).await?);
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
        }
    }

    async fn embed_one(&self, org_id: &str, text: &str, zdr: bool) -> anyhow::Result<Vec<f32>> {
        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1));
                tracing::warn!(attempt, ?backoff, "model-plane embed retry");
                tokio::time::sleep(backoff).await;
            }

            let request = self.build_request(org_id, text, zdr);
            let mut request = tonic::Request::new(request);
            if let Some(key) = self.internal_api_key.as_deref() {
                request
                    .metadata_mut()
                    .insert("x-api-key", MetadataValue::try_from(key)?);
            }

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

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "model-plane" | "modelplane" | "inference-core" | "inference_core" | "ai-core"
        | "ai_core" => "model_plane".to_string(),
        "azure" | "azure-openai" => "azure_openai".to_string(),
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
    }

    #[tokio::test]
    async fn model_plane_provider_selects_model_plane_backend() {
        let provider = EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            30_000,
            None,
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

    /// ModelPlane path: the ZDR signal is faithfully placed on the wire request
    /// (both true and false round-trip from the caller's argument).
    #[tokio::test]
    async fn model_plane_request_carries_zdr() {
        let provider = EmbeddingProvider::model_plane(
            "http://inference-core:9092",
            "text-embedding-3-large",
            "azure_openai",
            30_000,
            None,
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
    }
}
