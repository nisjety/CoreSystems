use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_database_url")]
    pub database_url: String,

    #[serde(default = "default_nats_url")]
    pub nats_url: String,

    #[serde(default = "default_admin_port")]
    pub admin_port: u16,

    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    // Extraction backend selector: "model_plane" (default — route LLM extraction
    // through inference-core's Infer RPC, honoring the no-independent-LLM-outside-
    // Model-Plane rule) or "azure_openai" (legacy direct-Azure fallback).
    #[serde(
        default = "default_extraction_provider",
        rename = "graph_extraction_provider",
        alias = "extraction_provider"
    )]
    pub extraction_provider: String,

    // Model Plane inference-core gRPC endpoint + routing hints for extraction.
    #[serde(default = "default_model_plane_grpc_url")]
    pub model_plane_ai_core_grpc_url: String,
    #[serde(default = "default_extraction_model")]
    pub model_plane_extraction_model: String,
    #[serde(default = "default_extraction_provider_hint")]
    pub model_plane_extraction_provider: String,
    #[serde(default = "default_extraction_timeout_ms")]
    pub model_plane_extraction_timeout_ms: u64,
    // Auth Core service-principal contract used for the inference-core gRPC
    // hop. The API key is deliberately empty by default so production startup
    // fails closed unless a deployment secret is supplied.
    #[serde(default = "default_model_plane_inference_token_url")]
    pub model_plane_inference_token_url: String,
    #[serde(default = "default_model_plane_inference_token_issuer")]
    pub model_plane_inference_token_issuer: String,
    #[serde(default = "default_model_plane_inference_service_id")]
    pub model_plane_inference_service_id: String,
    #[serde(default)]
    pub model_plane_inference_service_api_key: String,

    #[serde(default = "default_azure_endpoint")]
    pub azure_openai_endpoint: String,

    #[serde(default)]
    pub azure_openai_api_key: String,

    #[serde(default = "default_extraction_deployment")]
    pub azure_openai_extraction_deployment: String,

    #[serde(default = "default_max_entities_per_chunk")]
    pub max_entities_per_chunk: usize,

    #[serde(default = "default_community_min_size")]
    #[allow(dead_code)] // consumed by detect_communities once that path is wired
    pub community_min_size: usize,

    #[serde(default)]
    pub embedding_event_public_key_path: String,
    #[serde(default)]
    pub index_event_public_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        envy::from_env().map_err(|e| anyhow::anyhow!("config: {e}"))
    }
}

fn default_database_url() -> String {
    "postgres://dataplane:dataplane@localhost:5442/dataplane".into()
}

fn default_nats_url() -> String {
    "nats://localhost:4232".into()
}

fn default_admin_port() -> u16 {
    9203
}

fn default_grpc_port() -> u16 {
    // Matches the Model Plane gateway's DATAPLANE_GRAPH_ADDR default (:50053).
    50053
}

fn default_extraction_provider() -> String {
    "model_plane".into()
}

fn default_model_plane_grpc_url() -> String {
    "http://inference-core:9092".into()
}

fn default_extraction_model() -> String {
    "gpt-4o".into()
}

fn default_extraction_provider_hint() -> String {
    "azure_openai".into()
}

fn default_extraction_timeout_ms() -> u64 {
    60_000
}

fn default_model_plane_inference_token_url() -> String {
    "http://auth-core:3011/api/inference-core/internal-token".into()
}

fn default_model_plane_inference_token_issuer() -> String {
    "http://localhost:3011/api/convex-auth".into()
}

fn default_model_plane_inference_service_id() -> String {
    "graph-index".into()
}

fn default_azure_endpoint() -> String {
    String::new()
}

fn default_extraction_deployment() -> String {
    "gpt-4o".into()
}

fn default_max_entities_per_chunk() -> usize {
    20
}

fn default_community_min_size() -> usize {
    3
}

fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn graph_provider_uses_the_deployment_environment_contract() {
        let config: Config = envy::from_iter([(
            "GRAPH_EXTRACTION_PROVIDER".to_owned(),
            "azure_openai".to_owned(),
        )])
        .expect("config");

        assert_eq!(config.extraction_provider, "azure_openai");

        let legacy: Config =
            envy::from_iter([("EXTRACTION_PROVIDER".to_owned(), "azure_openai".to_owned())])
                .expect("legacy config");
        assert_eq!(legacy.extraction_provider, "azure_openai");
    }
}
