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
    #[serde(default = "default_model_plane_inference_token_url")]
    pub model_plane_inference_token_url: String,
    #[serde(default = "default_model_plane_inference_token_issuer")]
    pub model_plane_inference_token_issuer: String,
    #[serde(default)]
    pub model_plane_inference_service_id: String,
    #[serde(default)]
    pub model_plane_inference_service_api_key: String,
    /// Retention posture auth-core is configured to mint for this service's
    /// `inference-core` audience: `persistent` (zdr:false) or `zdr` (zdr:true).
    /// Must match the principal's `retentionByAudience` entry in
    /// `PLANE_SERVICE_PRINCIPALS_JSON`, or every mint fails closed.
    #[serde(default = "default_model_plane_inference_retention_posture")]
    pub model_plane_inference_retention_posture: String,

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
    /// Retrieve-wide / rerank-narrow window: how many fused candidates the
    /// cross-encoder scores per query. The six-arm fused list can run 150+
    /// deep; sending all of it sharded each query into several concurrent
    /// provider calls, which against the Foundry S0 per-second quota meant
    /// near-total 429 degradation (93/94 queries measured). At 50 — the
    /// client's parallel-split boundary — every query is exactly one provider
    /// call and one comparable scoring pass, and a candidate fused below rank
    /// 50 was not reaching the served top-10 anyway.
    #[serde(default = "default_rerank_top_k")]
    pub rerank_top_k: usize,
    // Rerank endpoint. Default = public Cohere. Set RERANK_ENDPOINT to an Azure
    // AI Foundry serverless Cohere rerank URL (and RERANK_USE_API_KEY=true) to
    // use the in-EU deployment instead of the public API.
    #[serde(default = "default_rerank_endpoint")]
    pub rerank_endpoint: String,
    #[serde(default)]
    pub rerank_use_api_key: bool,

    #[serde(default = "default_top_k")]
    pub retrieval_top_k: usize,
    #[serde(default = "default_top_n")]
    pub retrieval_top_n: usize,
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f32,
    #[serde(default = "default_true")]
    pub hybrid_enabled: bool,
    /// Smart hybrid — query-adaptive mode-mix suggestion (ON by default).
    /// Applies only when neither the request nor the agent config set an
    /// explicit blend; precedence: request > agent > smart > static defaults.
    #[serde(default = "default_true")]
    pub smart_hybrid_enabled: bool,
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
    /// RRF rank constant `k` in `1/(k+rank+1)` (plan P2-5). Was hardcoded at
    /// four fusion call sites; exposed so it can be tuned against the golden set
    /// without a rebuild. See `search::fusion::DEFAULT_RRF_K`.
    #[serde(default = "default_rrf_k")]
    pub rrf_k: f32,
    #[serde(default = "default_w_wiki")]
    pub w_wiki: f32,
    #[serde(default = "default_w_visual")]
    pub w_visual: f32,
    // Audio/video arms. Default 0.0 — SHADOW, not off-by-accident: these are the
    // newest and least-proven arms, and neither has been run against the golden
    // eval set. The visual arm launched at 0.05 only after eval; these stay at
    // zero until they earn a weight the same way. The arms still embed and
    // search when weighted, so flipping this is a config change, not a build.
    #[serde(default = "default_w_media")]
    pub w_audio: f32,
    #[serde(default = "default_w_media")]
    pub w_video: f32,
    /// Whether the box `media-embedder` runs on is Norwegian-operated
    /// infrastructure. Self-hosted does not imply sovereign — it only implies
    /// nothing egresses, which is a ZDR property, not a jurisdiction one.
    /// Defaults `false`: an unset/unproven claim is `Global`, never
    /// `Sovereign`, the same doctrine `Residency::classify` already applies
    /// to every Model Plane provider. Flip this only once the operator can
    /// actually name the datacenter and confirm it is Norwegian-operated —
    /// as of 2026-08-22 this has not been established (development has run
    /// on a laptop), so the safe default is load-bearing, not a placeholder.
    #[serde(default)]
    pub media_embedder_sovereign: bool,
    // Keyword arm — Meilisearch typo-tolerant exact-ID/code lookup. Small
    // default (0.05, same calibration as `w_visual` when it launched): a
    // new, unproven arm should start with a modest, provable contribution
    // rather than assume it belongs at parity with the established arms.
    // The arm no-op-skips when `MEILISEARCH_URL`/`MEILISEARCH_API_KEY` aren't
    // both configured, so a deployment without Meilisearch is unaffected.
    #[serde(default = "default_w_keyword")]
    pub w_keyword: f32,

    #[serde(default = "default_collection")]
    pub qdrant_collection: String,
    // Visual RAG arm — Cohere Embed v4 (Azure AI Foundry) page-image embeddings.
    // `w_visual` defaults 0.05 (ON by default; the arm no-op-skips when the
    // visual embedder isn't configured, so text-only deployments are unaffected).
    // The page-image collection is written by embedding-engine; the query
    // embedder hits the Embed v4 text route.
    #[serde(default = "default_visual_collection")]
    pub qdrant_visual_collection: String,
    #[serde(default = "default_visual_dim")]
    pub visual_embedding_dimension: usize,
    #[serde(default)]
    pub cohere_embed_v4_endpoint: String,
    #[serde(default)]
    pub cohere_embed_v4_api_key: String,
    #[serde(default = "default_embed_v4_deployment")]
    pub cohere_embed_v4_deployment: String,
    #[serde(default = "default_embed_v4_api_version")]
    pub cohere_embed_v4_api_version: String,

    // Audio + video arms, served by the self-hosted `media-embedder` sidecar
    // (LAION-CLAP for audio, X-CLIP or SigLIP 2 for video). Both arms are dark until
    // `media_embedder_endpoint` is set, exactly like the visual arm and the
    // ColQwen reranker — so an unset endpoint costs nothing and never errors.
    //
    // These are self-hosted because there is no cloud alternative, not as a
    // preference: Azure ships no audio-similarity embedding model, and its video
    // analyzer is extraction/description rather than a dense embedder. See
    // `docs/core-research/embedding-modality-and-rag-audit-2026-08-19.md` §3.
    #[serde(default)]
    pub media_embedder_endpoint: String,
    #[serde(default = "default_audio_collection")]
    pub qdrant_audio_collection: String,
    /// LAION-CLAP's projection dim. Must match the collection's configured size;
    /// swapping to GLAP (the audit's upgrade path) changes this.
    #[serde(default = "default_audio_dim")]
    pub audio_embedding_dimension: usize,
    #[serde(default = "default_video_collection")]
    pub qdrant_video_collection: String,
    /// Video tower projection dim: X-CLIP base-patch32 = 512 (the default),
    /// SigLIP 2 base/patch16-224 = 768. Must equal the indexing side — a width
    /// mismatch fails every search, not every write.
    #[serde(default = "default_video_dim")]
    pub video_embedding_dimension: usize,

    // ColQwen visual reranker (late-interaction MaxSim over Embed-v4's top-K
    // page-image candidates). ON by default, but a no-op until
    // `colqwen_endpoint_url` is set (the model runs as a separate GPU inference
    // server — local for verification, Hetzner/Azure for prod). When active,
    // the orchestrator reorders the visual candidates by ColQwen relevance; any
    // failure degrades to the Embed-v4 order (non-fatal).
    #[serde(default = "default_true")]
    pub visual_rerank_enabled: bool,
    #[serde(default)]
    pub colqwen_endpoint_url: String,
    #[serde(default = "default_visual_rerank_top_k")]
    pub visual_rerank_top_k: usize,
    /// Joint text-vs-image scoring (ON by default): ColQwen scores are mapped
    /// onto the fused score range so visual candidates interleave with text by
    /// relevance. `false` restores the band-preserving behavior (visual hits
    /// only reorder among themselves and can never leapfrog text).
    #[serde(default = "default_true")]
    pub joint_multimodal_rerank: bool,

    // Semantic *response* cache (Data-Plane-v2-owned vector tier for the
    // model-gateway SemanticCache seam). ON by default (CAG read path); the
    // scope-key fail-closed gate below still governs cross-principal sharing.
    #[serde(default = "default_true")]
    pub semantic_cache_enabled: bool,
    #[serde(default = "default_semantic_cache_collection")]
    pub semantic_cache_collection: String,
    #[serde(default = "default_semantic_cache_min_score")]
    pub semantic_cache_min_score: f32,
    #[serde(default = "default_semantic_cache_ttl_secs")]
    pub semantic_cache_ttl_secs: u64,
    /// Authz gate for the semantic cache. When true (default), a cache
    /// search/store with no `scope_key` fails closed (no-op) rather than risk
    /// serving one principal's grounded answer to another in the same org. Set
    /// false only for single-tenant / org-shared-only deployments to restore
    /// org-wide sharing.
    #[serde(default = "default_true")]
    pub semantic_cache_require_scope: bool,

    // Deep multi-hop graph arm — graph-index-rs's `/v1/graph/traverse` (Neo4j
    // read-model with server-side Postgres fallback), reached over HTTP with
    // the caller's verified bearer forwarded. Empty URL = remote path off; the
    // fused graph arm then uses its in-process 1-hop SQL grounding only.
    #[serde(default = "default_graph_index_url")]
    pub graph_index_url: String,
    #[serde(default = "default_graph_remote_timeout_ms")]
    pub graph_remote_timeout_ms: u64,
    #[serde(default = "default_graph_remote_max_hops")]
    pub graph_remote_max_hops: u8,

    #[serde(default = "default_sparse_search_backend")]
    pub sparse_search_backend: String,
    #[serde(default = "default_quickwit_url")]
    pub quickwit_url: String,
    #[serde(default = "default_quickwit_index_id")]
    pub quickwit_index_id: String,
    #[serde(default = "default_quickwit_search_timeout_ms")]
    pub quickwit_search_timeout_ms: u64,

    // Keyword arm's Meilisearch endpoint. Empty `meilisearch_api_key` (the
    // only field with no default — a master key must never have a checked-in
    // fallback) disables the arm: `search::keyword::MeilisearchQueryClient::
    // from_config` returns `None` and `arm_keyword` is never constructed.
    #[serde(default = "default_meilisearch_url")]
    pub meilisearch_url: String,
    #[serde(default)]
    pub meilisearch_api_key: String,
    #[serde(default = "default_meilisearch_index_uid")]
    pub meilisearch_index_uid: String,

    #[serde(default = "default_grpc_timeout")]
    pub grpc_timeout_secs: u32,
    #[serde(default = "default_grpc_max_concurrent")]
    pub grpc_max_concurrent: u32,

    #[serde(default)]
    pub user_core_service_token: Option<String>,

    #[serde(default)]
    pub documents_event_public_key_path: String,
    #[serde(default)]
    pub retrieval_event_private_key_path: String,
    #[serde(default = "default_event_auth_audience")]
    pub event_auth_audience: String,

    // gRPC TLS (env-gated). When both paths point to readable PEM files
    // AND the binary was built with the `grpc-tls` feature, the Tonic server
    // serves over TLS. Default (both empty) → plaintext, fine for cross-plane
    // traffic on the trusted `coresystem-local` docker network.
    #[serde(default)]
    pub grpc_tls_cert_path: Option<String>,
    #[serde(default)]
    pub grpc_tls_key_path: Option<String>,

    // Recency decay (P2-3). OFF by default: changes ranking for every query
    // and has not yet been measured against the P0.5 golden set. See
    // `pipeline::postprocess::RecencyDecay` for the decay curve and the
    // reasoning on why an unknown document_date is never penalized.
    #[serde(default)]
    pub recency_decay_enabled: bool,
    #[serde(default = "default_recency_decay_half_life_days")]
    pub recency_decay_half_life_days: f32,

    /// P2-7 — HyDE / query-expansion blend weight. `RetrievalRequest::
    /// query_expansion` is dead plumbing until a caller actually populates
    /// it (generating the hypothetical-document text is Model Plane's job,
    /// per Rule 7 — DPv2 must not run its own independent reasoning call to
    /// produce one); this only controls how much weight DPv2 gives it once
    /// supplied. No separate enabled flag: a caller populating this field is
    /// itself the opt-in, so gating it a second time server-side would just
    /// be redundant config surface. 0.0 = ignore the expansion entirely,
    /// 1.0 = embed only the expansion and ignore the literal query.
    #[serde(default = "default_query_expansion_blend_weight")]
    pub query_expansion_blend_weight: f32,
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
    // Canonical hyphen form: inference-core's provider registry id is
    // `azure-openai`. (inference-core now normalises `_`≡`-`, but we send the
    // canonical form so the hop never depends on that fallback — Phase 3 B-fix.)
    "azure-openai".into()
}
fn default_model_plane_embedding_timeout_ms() -> u64 {
    30_000
}
fn default_model_plane_inference_token_url() -> String {
    "http://auth-core:3011/api/inference-core/internal-token".into()
}
fn default_model_plane_inference_token_issuer() -> String {
    "http://localhost:3011/api/convex-auth".into()
}
/// Matches the deployed registry, which pins `retrieval-engine` ->
/// `inference-core` to `persistent`. Note this is the opposite of auth-core's
/// own default for an *unconfigured* audience (`zdr`), so tightening the
/// registry requires setting this variable too.
fn default_model_plane_inference_retention_posture() -> String {
    "persistent".into()
}
fn default_embedding_dim() -> usize {
    3072
}
fn default_rerank_top_k() -> usize {
    50
}
fn default_reranker_model() -> String {
    "rerank-english-v3.0".into()
}
fn default_rerank_endpoint() -> String {
    "https://api.cohere.ai/v1/rerank".into()
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
    0.45
}
fn default_w_bm25() -> f32 {
    0.2
}
fn default_w_graph() -> f32 {
    0.2
}
/// Single source of truth with the fusion module, so config and the algorithm
/// cannot drift.
fn default_rrf_k() -> f32 {
    crate::search::fusion::DEFAULT_RRF_K
}
fn default_w_wiki() -> f32 {
    0.1
}
fn default_collection() -> String {
    "dataplane_knowledge".into()
}
fn default_w_visual() -> f32 {
    // ON by default: sum with dense .45 + bm25 .2 + graph .2 + wiki .1 = 1.0.
    // The visual arm no-op-skips when no visual embedder is configured.
    0.05
}
fn default_w_keyword() -> f32 {
    // Same 0.05 calibration `w_visual` shipped with — small but present, so
    // an unproven new arm can be measured against the P0.5 golden set rather
    // than assumed. The arm no-op-skips when Meilisearch isn't configured.
    0.05
}
fn default_visual_collection() -> String {
    "dataplane_page_images".into()
}
fn default_visual_dim() -> usize {
    1536
}
fn default_visual_rerank_top_k() -> usize {
    20
}
fn default_recency_decay_half_life_days() -> f32 {
    180.0
}
fn default_query_expansion_blend_weight() -> f32 {
    0.5
}
fn default_w_media() -> f32 {
    0.0
}
fn default_audio_collection() -> String {
    "dataplane_audio_segments".into()
}
fn default_audio_dim() -> usize {
    512
}
fn default_video_collection() -> String {
    "dataplane_video_segments_siglip2".into()
}
fn default_video_dim() -> usize {
    // SigLIP 2 base/patch16-224 (the default tower) projects to 768.
    // X-CLIP base-patch32 would be 512 — switching towers means switching
    // BOTH this and the collection, which is why the collection names the tower.
    768
}
fn default_embed_v4_deployment() -> String {
    "Cohere-embed-4".into()
}
fn default_embed_v4_api_version() -> String {
    "2024-05-01-preview".into()
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
fn default_graph_index_url() -> String {
    // Compose-internal DNS; empty it out (GRAPH_INDEX_URL=) to disable the
    // remote deep-hop path outside the composed deployment.
    "http://graph-index:9203".into()
}
fn default_graph_remote_timeout_ms() -> u64 {
    // Kept under the retrieval p95<800ms gate budget: this arm overlaps the
    // dense/sparse round-trips via tokio::join!, but a slow-but-alive
    // graph-index must not drag the fused phase past the gate. The circuit
    // breaker (graph_remote.rs) skips the hop entirely after repeated failures.
    800
}
fn default_graph_remote_max_hops() -> u8 {
    3
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
fn default_meilisearch_url() -> String {
    "http://meilisearch:7700".into()
}
fn default_meilisearch_index_uid() -> String {
    "dataplane-knowledge".into()
}
fn default_grpc_timeout() -> u32 {
    30
}
fn default_grpc_max_concurrent() -> u32 {
    256
}
fn default_event_auth_audience() -> String {
    "dataplane-events".into()
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(envy::from_env::<Config>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_arm_default_weight_matches_visual_arms_launch_calibration() {
        // Both are "new, unproven arm" launches; §config docs above record
        // why 0.05 (not parity with dense/bm25/graph/wiki) is the right
        // starting point for either.
        assert_eq!(default_w_keyword(), 0.05);
        assert_eq!(default_w_keyword(), default_w_visual());
    }

    #[test]
    fn meilisearch_defaults_point_at_the_compose_internal_service_with_no_checked_in_key() {
        assert_eq!(default_meilisearch_url(), "http://meilisearch:7700");
        assert_eq!(default_meilisearch_index_uid(), "dataplane-knowledge");
        // No `default_meilisearch_api_key` function exists at all — the field
        // uses plain `#[serde(default)]` (empty string). A master key must
        // never have a checked-in fallback value.
    }

    #[test]
    fn minimal_env_deserializes_with_the_keyword_arm_defaulted_and_disableable() {
        let cfg: Config = envy::from_iter(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://test.invalid/test".to_string(),
            ),
            (
                "QDRANT_URL".to_string(),
                "http://test.invalid:6334".to_string(),
            ),
        ])
        .expect("minimal env must deserialize using field defaults");
        assert_eq!(cfg.w_keyword, 0.05);
        assert_eq!(cfg.meilisearch_url, "http://meilisearch:7700");
        // Empty by default — this is what makes the arm no-op-skip rather
        // than send unauthenticated requests when unconfigured.
        assert!(cfg.meilisearch_api_key.is_empty());
    }
}
