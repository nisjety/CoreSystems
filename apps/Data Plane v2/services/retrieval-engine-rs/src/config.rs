use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_grpc_port")]
    pub grpc_port: u16,

    pub database_url: String,
    pub qdrant_url: String,
    #[serde(default = "default_redis_url")]
    pub redis_url: String,

    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,
    #[serde(default = "default_model_plane_ai_core_grpc_url")]
    pub model_plane_ai_core_grpc_url: String,
    #[serde(default = "default_model_plane_embedding_provider")]
    pub model_plane_embedding_provider: String,
    #[serde(default = "default_model_plane_embedding_timeout_ms")]
    pub model_plane_embedding_timeout_ms: u64,

    #[serde(default)]
    pub azure_openai_api_key: String,
    #[serde(default)]
    pub azure_openai_endpoint: String,
    #[serde(default = "default_embedding_deployment")]
    pub azure_openai_embedding_deployment: String,
    #[serde(default = "default_embedding_dim")]
    pub embedding_dimension: usize,

    #[serde(default)]
    pub cohere_api_key: Option<String>,
    #[serde(default = "default_reranker_model")]
    pub reranker_model: String,

    #[serde(default = "default_top_k")]
    pub retrieval_top_k: usize,
    #[serde(default = "default_top_n")]
    pub retrieval_top_n: usize,
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f32,
    #[serde(default = "default_true")]
    pub hybrid_enabled: bool,
    #[serde(default = "default_bm25_weight")]
    #[allow(dead_code)] // legacy single-weight knob; superseded by w_bm25 in mode_mix
    pub bm25_weight: f32,

    // D4+D5 spec §7 default blend weights (dense + bm25 + graph + wiki).
    // Overridable per-request via `RetrievalRequest.mode_mix`.
    #[serde(default = "default_w_dense")]
    pub w_dense: f32,
    #[serde(default = "default_w_bm25")]
    pub w_bm25: f32,
    #[serde(default = "default_w_graph")]
    pub w_graph: f32,
    #[serde(default = "default_w_wiki")]
    pub w_wiki: f32,

    #[serde(default = "default_collection")]
    pub qdrant_collection: String,

    // Semantic *response* cache (Data-Plane-v2-owned vector tier for the
    // model-gateway SemanticCache seam). Opt-in via SEMANTIC_CACHE_ENABLED=true.
    #[serde(default)]
    pub semantic_cache_enabled: bool,
    #[serde(default = "default_semantic_cache_collection")]
    pub semantic_cache_collection: String,
    #[serde(default = "default_semantic_cache_min_score")]
    pub semantic_cache_min_score: f32,
    #[serde(default = "default_semantic_cache_ttl_secs")]
    pub semantic_cache_ttl_secs: u64,

    #[serde(default = "default_sparse_search_backend")]
    pub sparse_search_backend: String,
    #[serde(default = "default_quickwit_url")]
    pub quickwit_url: String,
    #[serde(default = "default_quickwit_index_id")]
    pub quickwit_index_id: String,
    #[serde(default = "default_quickwit_search_timeout_ms")]
    pub quickwit_search_timeout_ms: u64,

    #[serde(default = "default_grpc_timeout")]
    pub grpc_timeout_secs: u32,
    #[serde(default = "default_grpc_max_concurrent")]
    pub grpc_max_concurrent: u32,

    #[serde(default)]
    pub internal_api_key: Option<String>,

    // gRPC TLS (env-gated). When both paths point to readable PEM files
    // AND the binary was built with the `grpc-tls` feature, the Tonic server
    // serves over TLS. Default (both empty) → plaintext, fine for cross-plane
    // traffic on the trusted `aquatiq-local` docker network.
    #[serde(default)]
    pub grpc_tls_cert_path: Option<String>,
    #[serde(default)]
    pub grpc_tls_key_path: Option<String>,
}

fn default_http_port() -> u16 {
    8004
}
fn default_grpc_port() -> u16 {
    50052
}
fn default_redis_url() -> String {
    if let Ok(url) = std::env::var("DRAGONFLY_URL").or_else(|_| std::env::var("CACHE_URL")) {
        return url;
    }
    "redis://localhost:6379".into()
}
fn default_embedding_deployment() -> String {
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
fn default_embedding_dim() -> usize {
    3072
}
fn default_reranker_model() -> String {
    "rerank-english-v3.0".into()
}
fn default_top_k() -> usize {
    100
}
fn default_top_n() -> usize {
    10
}
fn default_confidence_threshold() -> f32 {
    0.35
}
fn default_true() -> bool {
    true
}
fn default_bm25_weight() -> f32 {
    0.3
}
fn default_w_dense() -> f32 {
    0.5
}
fn default_w_bm25() -> f32 {
    0.2
}
fn default_w_graph() -> f32 {
    0.2
}
fn default_w_wiki() -> f32 {
    0.1
}
fn default_collection() -> String {
    "dataplane_knowledge".into()
}
fn default_semantic_cache_collection() -> String {
    "semantic_response_cache".into()
}
fn default_semantic_cache_min_score() -> f32 {
    0.95
}
fn default_semantic_cache_ttl_secs() -> u64 {
    86_400
}
fn default_sparse_search_backend() -> String {
    "postgres".into()
}
fn default_quickwit_url() -> String {
    "http://quickwit:7280".into()
}
fn default_quickwit_index_id() -> String {
    "dataplane-corpus".into()
}
fn default_quickwit_search_timeout_ms() -> u64 {
    1500
}
fn default_grpc_timeout() -> u32 {
    30
}
fn default_grpc_max_concurrent() -> u32 {
    256
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}
