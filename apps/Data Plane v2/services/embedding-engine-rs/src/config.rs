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
    /// Requested residency region forwarded to inference-core on every embedding
    /// request (from `EMBEDDING_REGION`). inference-core enforces the EU
    /// residency gate deny-by-default; defaulting to an EU region keeps the
    /// embedding path inside the EU residency boundary. Empty = no preference.
    #[serde(default = "default_embedding_region")]
    pub embedding_region: String,
    #[serde(default = "default_dim")]
    pub embedding_dimension: u64,

    #[serde(default = "default_collection")]
    pub qdrant_collection: String,

    // Visual RAG arm — Cohere Embed v4 (Azure AI Foundry) multimodal image
    // embeddings. When `cohere_embed_v4_endpoint` is set, the page-image consumer
    // embeds rendered pages into `qdrant_visual_collection`. Unset → visual arm
    // disabled (text-only deployment).
    #[serde(default)]
    pub cohere_embed_v4_endpoint: String,
    #[serde(default)]
    pub cohere_embed_v4_api_key: String,
    #[serde(default = "default_embed_v4_deployment")]
    pub cohere_embed_v4_deployment: String,
    #[serde(default = "default_embed_v4_api_version")]
    pub cohere_embed_v4_api_version: String,
    #[serde(default = "default_visual_collection")]
    pub qdrant_visual_collection: String,
    #[serde(default = "default_visual_dim")]
    pub visual_embedding_dimension: u64,

    #[serde(default = "default_admin_port")]
    pub admin_port: u16,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_delivery")]
    pub max_delivery_attempts: u32,

    #[serde(default)]
    pub index_event_public_key_path: String,
    #[serde(default)]
    pub wiki_event_public_key_path: String,
    #[serde(default)]
    pub embedding_event_private_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,
}

fn default_deployment() -> String {
    "text-embedding-3-large".into()
}
fn default_embedding_region() -> String {
    // EU-by-default: keep embeddings inside the EU residency boundary unless an
    // operator explicitly overrides EMBEDDING_REGION (and accepts non-EU on
    // inference-core via MODEL_PLANE_ALLOW_NON_EU_EMBEDDING).
    "swedencentral".into()
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
fn default_embed_v4_deployment() -> String {
    "Cohere-embed-4".into()
}
fn default_embed_v4_api_version() -> String {
    "2024-05-01-preview".into()
}
fn default_visual_collection() -> String {
    "dataplane_page_images".into()
}
fn default_visual_dim() -> u64 {
    1536
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
fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
