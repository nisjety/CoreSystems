use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;

mod service_auth;
pub mod visual;

use service_auth::{InferenceBearer, InferenceTokenClient};

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
            }),
            "azure_openai" => Self::azure_openai(
                &cfg.azure_openai_endpoint,
                &cfg.azure_openai_api_key,
                &cfg.azure_openai_embedding_deployment,
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
                "unsupported EMBEDDING_PROVIDER `{other}`; expected `model_plane` or `azure_openai`"
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
            EmbeddingBackend::DeterministicTest { .. } => "deterministic_test",
        }
    }

    pub fn model_name(&self) -> &str {
        match &self.inner {
            EmbeddingBackend::ModelPlane(client) => &client.model,
            EmbeddingBackend::AzureOpenAi(client) => &client.deployment,
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

fn sanitized_provider_status_error(provider: &str, status: reqwest::StatusCode) -> anyhow::Error {
    anyhow::anyhow!("{provider} returned HTTP status {}", status.as_u16())
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
