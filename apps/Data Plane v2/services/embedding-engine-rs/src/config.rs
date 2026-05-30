use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub qdrant_url: String,

    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,
    #[serde(default = "default_model_plane_ai_core_grpc_url")]
    pub model_plane_ai_core_grpc_url: String,
    #[serde(default = "default_model_plane_embedding_provider")]
    pub model_plane_embedding_provider: String,
    #[serde(default = "default_model_plane_embedding_timeout_ms")]
    pub model_plane_embedding_timeout_ms: u64,
    #[serde(default)]
    pub internal_api_key: Option<String>,

    #[serde(default)]
    pub azure_openai_api_key: String,
    #[serde(default)]
    pub azure_openai_endpoint: String,
    #[serde(default = "default_deployment")]
    pub azure_openai_embedding_deployment: String,
    #[serde(default = "default_dim")]
    pub embedding_dimension: u64,

    #[serde(default = "default_collection")]
    pub qdrant_collection: String,
    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_delivery")]
    pub max_delivery_attempts: u32,
}

fn default_deployment() -> String {
    "text-embedding-3-large".into()
}
fn default_embedding_provider() -> String {
    "model_plane".into()
}
fn default_model_plane_ai_core_grpc_url() -> String {
    "http://inference-core:9092".into()
}
fn default_model_plane_embedding_provider() -> String {
    "azure_openai".into()
}
fn default_model_plane_embedding_timeout_ms() -> u64 {
    30_000
}
fn default_dim() -> u64 {
    3072
}
fn default_collection() -> String {
    "dataplane_knowledge".into()
}
fn default_admin_port() -> u16 {
    9202
}
fn default_batch_size() -> usize {
    32
}
fn default_max_delivery() -> u32 {
    5
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
