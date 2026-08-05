use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::config::Config;
use crate::inference_auth::{InferenceTokenClient, RetentionPosture};
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
    token_client: InferenceTokenClient,
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
                let retention_posture =
                    RetentionPosture::parse(&cfg.model_plane_inference_retention_posture)?;
                let token_client = if standalone_startup() {
                    InferenceTokenClient::new_allow_unconfigured(
                        &cfg.model_plane_inference_token_url,
                        &cfg.model_plane_inference_token_issuer,
                        &cfg.model_plane_inference_service_id,
                        &cfg.model_plane_inference_service_api_key,
                        retention_posture,
                    )?
                } else {
                    InferenceTokenClient::new(
                        &cfg.model_plane_inference_token_url,
                        &cfg.model_plane_inference_token_issuer,
                        &cfg.model_plane_inference_service_id,
                        &cfg.model_plane_inference_service_api_key,
                        retention_posture,
                    )?
                };
                Backend::ModelPlane(Box::new(ModelPlaneExtractor {
                    client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                    model: cfg.model_plane_extraction_model.clone(),
                    provider: cfg.model_plane_extraction_provider.clone(),
                    timeout,
                    token_client,
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

    pub async fn extract(
        &self,
        text: &str,
        org_id: &str,
        zdr: bool,
    ) -> anyhow::Result<ExtractionResult> {
        let prompt = build_prompt(self.max_entities, text);
        let content = match &self.backend {
            Backend::ModelPlane(inner) => inner.infer(org_id, &prompt, zdr).await?,
            Backend::AzureOpenAi(inner) => {
                if zdr {
                    anyhow::bail!("ZDR graph extraction must not egress to the direct-Azure path");
                }
                inner.chat(&prompt).await?
            }
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

    /// Summarises one GraphRAG community from its member entities (plan P1-5).
    ///
    /// This is GraphRAG's "global search" input: a short thematic description of
    /// a cluster, which answers questions no single chunk can ("what areas does
    /// this organisation work in?"). `graph_communities.summary` and
    /// `retrieval-engine`'s `community_summary_search` already existed; only the
    /// generator was missing.
    ///
    /// Reuses the exact same backend dispatch and ZDR posture as [`extract`] —
    /// including the hard refusal to egress restrictive content to the direct
    /// Azure path. In practice graph entities only exist for non-restrictive
    /// content (the ingest consumer drops restrictive-ZDR events before
    /// extraction), so `zdr` is expected to be false here; the guard stays so
    /// that assumption cannot silently rot.
    ///
    /// Returns prose, not JSON — there is nothing to parse, so a provider that
    /// wraps or chats around the answer degrades to a slightly noisier summary
    /// rather than a hard failure.
    pub async fn summarize_community(
        &self,
        entity_labels: &[String],
        org_id: &str,
        zdr: bool,
    ) -> anyhow::Result<String> {
        if entity_labels.is_empty() {
            anyhow::bail!("cannot summarise a community with no member entities");
        }
        let prompt = build_community_summary_prompt(entity_labels);
        let content = match &self.backend {
            Backend::ModelPlane(inner) => inner.infer(org_id, &prompt, zdr).await?,
            Backend::AzureOpenAi(inner) => {
                if zdr {
                    anyhow::bail!(
                        "ZDR community summarisation must not egress to the direct-Azure path"
                    );
                }
                inner.chat(&prompt).await?
            }
        };
        let summary = content.trim().to_string();
        if summary.is_empty() {
            anyhow::bail!("community summary came back empty");
        }
        Ok(summary)
    }
}

/// Caps how many member labels are sent for one summary.
///
/// A community can span hundreds of entities; sending all of them would blow the
/// prompt budget and cost for no gain, since a thematic summary is determined by
/// the dominant members. Labels arrive ordered by the caller (highest-confidence
/// first), so truncation drops the weakest.
const MAX_SUMMARY_LABELS: usize = 60;

fn build_community_summary_prompt(entity_labels: &[String]) -> String {
    let shown: Vec<&str> = entity_labels
        .iter()
        .take(MAX_SUMMARY_LABELS)
        .map(String::as_str)
        .collect();
    let omitted = entity_labels.len().saturating_sub(shown.len());
    let more = if omitted > 0 {
        format!("\n(and {omitted} further related entities)")
    } else {
        String::new()
    };
    format!(
        r#"These entities form one cluster in a knowledge graph built from an organisation's own documents:

{labels}{more}

Write 2-3 sentences describing what this cluster is about, as a factual topic
summary someone could use to decide whether it is relevant to their question.

Rules:
- Use ONLY the entities listed. Do not add facts, figures, or claims that are not present.
- If the entities are too disparate to share a theme, say exactly that.
- Answer in the dominant language of the entity names.
- Return the summary text only — no preamble, no headings, no bullet points."#,
        labels = shown.join("\n"),
        more = more,
    )
}

fn standalone_startup() -> bool {
    std::env::var("APP_ENV")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("standalone"))
        .unwrap_or(false)
}

impl ModelPlaneExtractor {
    /// Build the Infer request. Split out so a unit test can assert the prompt +
    /// routing hints are carried onto the wire without a live inference-core.
    fn build_request(
        &self,
        org_id: &str,
        prompt: &str,
        zdr: bool,
    ) -> model_plane::v1::InferRequest {
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
            zdr,
            tools: vec![],
            tool_choice: String::new(),
        }
    }

    fn build_authenticated_request(
        &self,
        bearer: &str,
        org_id: &str,
        prompt: &str,
        zdr: bool,
    ) -> anyhow::Result<tonic::Request<model_plane::v1::InferRequest>> {
        anyhow::ensure!(
            !bearer.trim().is_empty()
                && bearer == bearer.trim()
                && !bearer.chars().any(char::is_whitespace),
            "verified bearer is required"
        );
        anyhow::ensure!(
            !org_id.trim().is_empty() && org_id == org_id.trim(),
            "org_id is required"
        );

        let mut request = tonic::Request::new(self.build_request(org_id, prompt, zdr));
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {bearer}"))?,
        );
        Ok(request)
    }

    async fn infer(&self, org_id: &str, prompt: &str, zdr: bool) -> anyhow::Result<String> {
        let bearer = self.token_client.mint(org_id).await?;
        let request = self.build_authenticated_request(bearer.as_str(), org_id, prompt, zdr)?;
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
    // `entity_type` is constrained to the closed ontology (P1-3). Left free, the
    // extractor produced 61 distinct types across 547 entities — including
    // `Aquatiq` as both `Organization` and `Company`, which split one company
    // into two unrelated graph nodes.
    //
    // This prompt only *steers*: there is no provider-side enum here (the schema
    // is prose, and the Anthropic path drops structured-output schemas), so
    // `store::canonical_entity_type` remains the authoritative gate and maps
    // anything unrecognised to `Concept`.
    format!(
        r#"Extract entities, relationships, and claims from the following text.
Return JSON with this schema:
{{
  "entities": [{{ "entity_type": "...", "entity_text": "...", "confidence": 0.0-1.0 }}],
  "relationships": [{{ "source_entity": "...", "target_entity": "...", "relation_type": "...", "confidence": 0.0-1.0 }}],
  "claims": [{{ "claim_text": "...", "related_entities": ["..."], "confidence": 0.0-1.0 }}]
}}

"entity_type" MUST be exactly one of these {type_count} values — never invent another:
{types}

Pick the closest match. Use "Concept" for anything abstract that fits no other
type (an industry, a field, a risk, a property). Do not emit "Claim" as an
entity type; assertions belong in "claims".

Max {max} entities. Only return valid JSON.

Text:
{text}"#,
        type_count = crate::store::ENTITY_TYPES.len(),
        types = crate::store::ENTITY_TYPES.join(", "),
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

    #[test]
    fn community_summary_prompt_lists_members_and_forbids_invention() {
        let labels = vec![
            "Aquatiq".to_string(),
            "Hygieneanlegg".to_string(),
            "Næringsmiddelindustri".to_string(),
        ];
        let p = build_community_summary_prompt(&labels);
        for l in &labels {
            assert!(p.contains(l.as_str()), "prompt must list {l}");
        }
        // Grounding rules: the summary must not become a source of new facts,
        // and it must not silently invent a theme for unrelated members.
        assert!(p.contains("ONLY the entities listed"));
        assert!(p.contains("too disparate"));
        assert!(p.contains("dominant language"), "corpus is NO+EN mixed");
        assert!(!p.contains("further related entities"), "nothing omitted at n=3");
    }

    #[test]
    fn community_summary_prompt_truncates_and_says_so() {
        // A community can span hundreds of entities; the prompt must cap them
        // and disclose the omission rather than silently dropping members.
        let labels: Vec<String> = (0..MAX_SUMMARY_LABELS + 25)
            .map(|i| format!("entity-{i}"))
            .collect();
        let p = build_community_summary_prompt(&labels);
        assert!(p.contains("entity-0"), "highest-ranked member kept");
        assert!(
            !p.contains(&format!("entity-{}", MAX_SUMMARY_LABELS + 24)),
            "weakest member beyond the cap must be dropped"
        );
        assert!(
            p.contains("and 25 further related entities"),
            "truncation must be disclosed to the model"
        );
    }

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
            model_plane_inference_retention_posture: "persistent".to_string(),
            model_plane_inference_token_url:
                "http://auth-core:3011/api/inference-core/internal-token".to_string(),
            model_plane_inference_token_issuer: "http://auth-core:3011/api/convex-auth".to_string(),
            model_plane_inference_service_id: "graph-index".to_string(),
            model_plane_inference_service_api_key: "isolated-test-service-credential".to_string(),
            azure_openai_endpoint: String::new(),
            azure_openai_api_key: String::new(),
            azure_openai_extraction_deployment: "gpt-4o".to_string(),
            max_entities_per_chunk: 20,
            community_min_size: 3,
            neo4j_enabled: false,
            neo4j_url: "bolt://neo4j:7687".to_string(),
            neo4j_user: "neo4j".to_string(),
            neo4j_password: String::new(),
            neo4j_database: "neo4j".to_string(),
            neo4j_boot_attempts: 30,
            graph_max_hops: 3,
            graph_traverse_max_entities: 100,
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

    #[tokio::test]
    async fn model_plane_requires_service_principal_credential() {
        let mut config = test_config("model_plane");
        config.model_plane_inference_service_api_key.clear();
        let error = match GraphExtractor::new(&config) {
            Ok(_) => panic!("model-plane startup must fail closed without a credential"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("SERVICE_API_KEY"));
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

    #[tokio::test]
    async fn azure_backend_rejects_zdr_before_network_egress() {
        let mut config = test_config("azure_openai");
        config.azure_openai_endpoint = "http://127.0.0.1:1/unreachable".to_string();
        config.azure_openai_api_key = "isolated-test-key".to_string();
        let extractor = GraphExtractor::new(&config).expect("Azure extractor");
        let error = extractor
            .extract("restricted text", "org-9", true)
            .await
            .expect_err("ZDR must fail before a retaining provider call");
        assert!(error.to_string().contains("must not egress"));
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
        let req = inner.build_request("org-9", &prompt, false);
        assert_eq!(req.org_id, "org-9");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.provider_hint, "azure_openai");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
        assert!(req.messages[1].content.contains("some chunk text"));
        assert!(req.messages[1].content.contains("Only return valid JSON"));
    }

    #[tokio::test]
    async fn model_plane_authenticated_request_uses_bearer_exact_org_and_zdr_without_shared_key() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let Backend::ModelPlane(inner) = &extractor.backend else {
            panic!("expected model-plane backend");
        };
        let prompt = build_prompt(20, "isolated test text");
        let request = inner
            .build_authenticated_request("header.payload.signature", "org-9", &prompt, true)
            .expect("authenticated request");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer header.payload.signature")
        );
        assert!(request.metadata().get("x-api-key").is_none());
        assert_eq!(request.get_ref().org_id, "org-9");
        assert!(
            request.get_ref().zdr,
            "restrictive ZDR posture must survive wrapping"
        );

        let non_zdr = inner
            .build_authenticated_request("header.payload.signature", "org-9", &prompt, false)
            .expect("authenticated request");
        assert!(
            !non_zdr.get_ref().zdr,
            "caller ZDR posture must not be overwritten"
        );
    }

    #[tokio::test]
    async fn model_plane_authenticated_request_fails_closed_without_bearer_or_tenant() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let Backend::ModelPlane(inner) = &extractor.backend else {
            panic!("expected model-plane backend");
        };
        let prompt = build_prompt(20, "isolated test text");

        for bearer in ["", "  ", "header payload"] {
            let error = inner
                .build_authenticated_request(bearer, "org-9", &prompt, false)
                .expect_err("malformed bearer must fail closed");
            assert!(error.to_string().contains("bearer"));
        }
        let error = inner
            .build_authenticated_request("header.payload.signature", "", &prompt, false)
            .expect_err("missing tenant must fail closed");
        assert!(error.to_string().contains("org_id"));
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
