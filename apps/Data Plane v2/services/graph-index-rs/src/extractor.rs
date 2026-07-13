use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;
use crate::model::ExtractionResult;

// Model Plane inference client (compiled by build.rs from the shared
// InferenceCore proto). Graph LLM extraction routes through inference-core's
// Infer RPC by default instead of calling Azure OpenAI directly.
mod model_plane {
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

const SYSTEM_PROMPT: &str =
    "You are a knowledge graph extraction engine. Extract entities, relationships, and claims. Return only valid JSON.";

pub struct GraphExtractor {
    backend: Backend,
    max_entities: usize,
}

enum Backend {
    // Boxed: the model-plane client (gRPC channel + routing config) is the larger
    // variant; boxing keeps the enum small (clippy::large_enum_variant).
    ModelPlane(Box<ModelPlaneExtractor>),
    AzureOpenAi(AzureExtractor),
}

struct ModelPlaneExtractor {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    model: String,
    provider: String,
    timeout: Duration,
    internal_api_key: Option<String>,
}

struct AzureExtractor {
    client: Client,
    endpoint: String,
    api_key: String,
    deployment: String,
}

impl GraphExtractor {
    pub fn new(cfg: &Config) -> anyhow::Result<Self> {
        let backend = match normalize_provider(&cfg.extraction_provider).as_str() {
            "model_plane" => {
                let timeout = Duration::from_millis(cfg.model_plane_extraction_timeout_ms.max(1));
                let channel = Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
                    .with_context(|| {
                        format!(
                            "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                            cfg.model_plane_ai_core_grpc_url
                        )
                    })?
                    .connect_timeout(timeout)
                    .timeout(timeout)
                    .connect_lazy();
                Backend::ModelPlane(Box::new(ModelPlaneExtractor {
                    client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                    model: cfg.model_plane_extraction_model.clone(),
                    provider: cfg.model_plane_extraction_provider.clone(),
                    timeout,
                    internal_api_key: cfg.internal_api_key.clone().filter(|k| !k.is_empty()),
                }))
            }
            "azure_openai" => {
                if cfg.azure_openai_endpoint.trim().is_empty() {
                    anyhow::bail!(
                        "AZURE_OPENAI_ENDPOINT is required when GRAPH_EXTRACTION_PROVIDER=azure_openai"
                    );
                }
                Backend::AzureOpenAi(AzureExtractor {
                    client: Client::new(),
                    endpoint: cfg.azure_openai_endpoint.trim_end_matches('/').to_string(),
                    api_key: cfg.azure_openai_api_key.clone(),
                    deployment: cfg.azure_openai_extraction_deployment.clone(),
                })
            }
            other => anyhow::bail!(
                "unsupported GRAPH_EXTRACTION_PROVIDER `{other}`; expected `model_plane` or `azure_openai`"
            ),
        };
        Ok(Self {
            backend,
            max_entities: cfg.max_entities_per_chunk,
        })
    }

    pub fn provider_name(&self) -> &'static str {
        match self.backend {
            Backend::ModelPlane(_) => "model_plane",
            Backend::AzureOpenAi(_) => "azure_openai",
        }
    }

    pub async fn extract(&self, text: &str, org_id: &str) -> anyhow::Result<ExtractionResult> {
        let prompt = build_prompt(self.max_entities, text);
        let content = match &self.backend {
            Backend::ModelPlane(inner) => inner.infer(org_id, &prompt).await?,
            Backend::AzureOpenAi(inner) => inner.chat(&prompt).await?,
        };
        let result = parse_extraction(&content)?;
        tracing::info!(
            org_id,
            provider = self.provider_name(),
            entities = result.entities.len(),
            relationships = result.relationships.len(),
            claims = result.claims.len(),
            "extraction complete"
        );
        Ok(result)
    }
}

impl ModelPlaneExtractor {
    /// Build the Infer request. Split out so a unit test can assert the prompt +
    /// routing hints are carried onto the wire without a live inference-core.
    fn build_request(&self, org_id: &str, prompt: &str) -> model_plane::v1::InferRequest {
        model_plane::v1::InferRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            model: self.model.clone(),
            provider_hint: self.provider.clone(),
            messages: vec![
                model_plane::v1::ChatMessage {
                    role: "system".to_string(),
                    content: SYSTEM_PROMPT.to_string(),
                    name: String::new(),
                },
                model_plane::v1::ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                    name: String::new(),
                },
            ],
            temperature: 0.1,
            max_tokens: 4096,
            structured_output_schema: String::new(),
            // Graph extraction does not currently thread per-document ZDR
            // classification; inference-core still enforces the EU residency gate
            // on every call regardless of this flag.
            zdr: false,
            tools: vec![],
            tool_choice: String::new(),
        }
    }

    async fn infer(&self, org_id: &str, prompt: &str) -> anyhow::Result<String> {
        let mut request = tonic::Request::new(self.build_request(org_id, prompt));
        if let Some(key) = self.internal_api_key.as_deref() {
            request
                .metadata_mut()
                .insert("x-api-key", MetadataValue::try_from(key)?);
        }
        let mut client = self.client.clone();
        let resp = tokio::time::timeout(self.timeout, client.infer(request))
            .await
            .map_err(|_| {
                anyhow::anyhow!("model-plane inference timed out after {:?}", self.timeout)
            })?
            .map_err(|status| anyhow::anyhow!(status).context("model-plane inference failed"))?;
        let body = resp.into_inner();
        if body.content.trim().is_empty() {
            anyhow::bail!("model-plane inference returned empty content");
        }
        Ok(body.content)
    }
}

impl AzureExtractor {
    async fn chat(&self, prompt: &str) -> anyhow::Result<String> {
        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
            self.endpoint, self.deployment
        );
        let body = serde_json::json!({
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.1,
            "max_tokens": 4096,
            "response_format": {"type": "json_object"}
        });

        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("extraction API call")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("extraction API {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        Ok(json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}")
            .to_string())
    }
}

fn build_prompt(max_entities: usize, text: &str) -> String {
    format!(
        r#"Extract entities, relationships, and claims from the following text.
Return JSON with this schema:
{{
  "entities": [{{ "entity_type": "...", "entity_text": "...", "confidence": 0.0-1.0 }}],
  "relationships": [{{ "source_entity": "...", "target_entity": "...", "relation_type": "...", "confidence": 0.0-1.0 }}],
  "claims": [{{ "claim_text": "...", "related_entities": ["..."], "confidence": 0.0-1.0 }}]
}}
Max {max} entities. Only return valid JSON.

Text:
{text}"#,
        max = max_entities,
        text = text
    )
}

/// Parse an extraction result, tolerating a ```json fenced``` wrapper — the Infer
/// path does not force a JSON response format, so some providers wrap output.
fn parse_extraction(content: &str) -> anyhow::Result<ExtractionResult> {
    let cleaned = strip_json_fences(content);
    serde_json::from_str(&cleaned).context("parse extraction result")
}

fn strip_json_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches(['\n', '\r']);
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "model-plane" | "modelplane" | "inference-core" | "inference_core" | "ai-core"
        | "ai_core" => "model_plane".to_string(),
        "azure" | "azure-openai" => "azure_openai".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(provider: &str) -> Config {
        Config {
            database_url: String::new(),
            nats_url: String::new(),
            admin_port: 9203,
            grpc_port: 50053,
            extraction_provider: provider.to_string(),
            model_plane_ai_core_grpc_url: "http://inference-core:9092".to_string(),
            model_plane_extraction_model: "gpt-4o".to_string(),
            model_plane_extraction_provider: "azure_openai".to_string(),
            model_plane_extraction_timeout_ms: 60_000,
            internal_api_key: None,
            azure_openai_endpoint: String::new(),
            azure_openai_api_key: String::new(),
            azure_openai_extraction_deployment: "gpt-4o".to_string(),
            max_entities_per_chunk: 20,
            community_min_size: 3,
            embedding_event_public_key_path: String::new(),
            index_event_public_key_path: String::new(),
            event_auth_audience: "dataplane-events".to_string(),
        }
    }

    #[tokio::test]
    async fn new_defaults_to_model_plane() {
        let extractor =
            GraphExtractor::new(&test_config("model_plane")).expect("model-plane extractor");
        assert_eq!(extractor.provider_name(), "model_plane");
    }

    #[test]
    fn azure_backend_requires_endpoint() {
        let err = match GraphExtractor::new(&test_config("azure_openai")) {
            Ok(_) => panic!("empty Azure endpoint must fail"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("AZURE_OPENAI_ENDPOINT"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn normalize_provider_aliases() {
        assert_eq!(normalize_provider("inference-core"), "model_plane");
        assert_eq!(normalize_provider("Model_Plane"), "model_plane");
        assert_eq!(normalize_provider("azure-openai"), "azure_openai");
    }

    #[tokio::test]
    async fn model_plane_request_carries_prompt_and_routing() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let Backend::ModelPlane(inner) = &extractor.backend else {
            panic!("expected model-plane backend");
        };
        let prompt = build_prompt(20, "some chunk text");
        let req = inner.build_request("org-9", &prompt);
        assert_eq!(req.org_id, "org-9");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.provider_hint, "azure_openai");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
        assert!(req.messages[1].content.contains("some chunk text"));
        assert!(req.messages[1].content.contains("Only return valid JSON"));
    }

    #[test]
    fn parse_extraction_handles_fenced_and_bare_json() {
        let bare = r#"{"entities":[{"entity_type":"org","entity_text":"Acme","confidence":0.9}],"relationships":[],"claims":[]}"#;
        let parsed = parse_extraction(bare).expect("bare json");
        assert_eq!(parsed.entities.len(), 1);

        let fenced = format!("```json\n{bare}\n```");
        let parsed2 = parse_extraction(&fenced).expect("fenced json");
        assert_eq!(parsed2.entities.len(), 1);
    }
}
