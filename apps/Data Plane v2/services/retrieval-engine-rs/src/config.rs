use std::time::Duration;

use serde::Deserialize;

use crate::search::rerank::{BreakerPolicy, ProviderLimits};

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

    // Z-41 — egress attestation for ZDR-restricted content, read by `egress_attestation`. Data, not
    // code: every field empty (the default) attests nothing, so a ZDR query answers lexically with a
    // notice and the embedding guards refuse before any network call. The six fields of one Azure
    // resource are set together, through the release path. A partial or invalid set is logged as a
    // boot problem; it never stops the engine.
    #[serde(default)]
    pub azure_cohere_zdr_resource_id: String,
    #[serde(default)]
    pub azure_cohere_zdr_approval_ref: String,
    #[serde(default)]
    pub azure_cohere_zdr_effective_date: String,
    #[serde(default)]
    pub azure_cohere_zdr_review_by: String,
    #[serde(default)]
    pub azure_cohere_zdr_reviewer: String,
    #[serde(default)]
    pub azure_cohere_zdr_digest: String,
    #[serde(default)]
    pub azure_openai_zdr_resource_id: String,
    #[serde(default)]
    pub azure_openai_zdr_approval_ref: String,
    #[serde(default)]
    pub azure_openai_zdr_effective_date: String,
    #[serde(default)]
    pub azure_openai_zdr_review_by: String,
    #[serde(default)]
    pub azure_openai_zdr_reviewer: String,
    #[serde(default)]
    pub azure_openai_zdr_digest: String,

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

    /// QB-T05a — `POST /v1/rerank` (Quarry option B). Read as text: envy accepts only
    /// `true`/`false`, while the compose contract writes `0`/`1`. Off until the live proof.
    /// Use [`Config::web_rerank_on`]; a malformed value reads as off and is logged at boot.
    #[serde(default = "default_switch_off")]
    pub web_rerank_enabled: String,
    /// QB-T05a — `WEB_RERANK_PROCESSOR_APPROVED`, the processor gate (GDPR Art. 28). Off
    /// until counsel confirms it; the live proof sets it in its own environment only.
    #[serde(default = "default_switch_off")]
    pub web_rerank_processor_approved: String,
    /// QB-T05a — comma-separated service ids that may call `POST /v1/rerank`.
    #[serde(default = "default_web_rerank_allowed_services")]
    pub web_rerank_allowed_services: String,
    /// QB-T05a — alert threshold for billed web-rerank units per org per UTC day. Alert
    /// only: exceeding it logs and counts, it never stops a call. `0` disables the alert.
    #[serde(default = "default_web_rerank_daily_unit_budget")]
    pub web_rerank_daily_unit_budget: u64,
    /// QB-M — `USAGE_EMIT_ENABLED`, the usage ledger's switch, read as text like the others. Off
    /// (the default) means the web route answers `skipped metering_unavailable` and calls no paid
    /// provider: a call that cannot be recorded is not made. Use [`Config::usage_emit_on`]; a
    /// malformed value reads as off and is logged at boot.
    #[serde(default = "default_switch_off")]
    pub usage_emit_enabled: String,

    /// QB-T05b — concurrent provider calls this replica may have in flight, shared by tenant
    /// retrieval and `POST /v1/rerank`. Text, like the switches. `0` (default) leaves tenant
    /// retrieval unbounded and gives web rerank no capacity. See [`Config::provider_limits`].
    #[serde(default = "default_zero_setting")]
    pub rerank_provider_max_inflight: String,
    /// QB-T05b — provider requests per second this replica may send. `0` (default) gives web
    /// rerank no capacity. Tenant retrieval is never refused by it, but its calls spend it.
    #[serde(default = "default_zero_setting")]
    pub rerank_provider_rps: String,
    /// QB-T05b — how long a tenant rerank waits for a permit once the pool is saturated, in
    /// milliseconds. Past it the call degrades to the fused order, as any reranker failure does.
    #[serde(default = "default_rerank_provider_tenant_wait_ms")]
    pub rerank_provider_tenant_wait_ms: String,
    /// QB-T05b — the provider breaker: failed calls inside the window that open it.
    #[serde(default = "default_rerank_breaker_failures")]
    pub rerank_breaker_failures: String,
    /// QB-T05b — the breaker's failure window, in seconds.
    #[serde(default = "default_rerank_breaker_window_secs")]
    pub rerank_breaker_window_secs: String,
    /// QB-T05b — how long the breaker stays open before one probe, in seconds.
    #[serde(default = "default_rerank_breaker_open_secs")]
    pub rerank_breaker_open_secs: String,
    /// CR-T15 — `RERANK_INPUT_INCLUDE_CONTEXT`, the rerank input switch, read as text like the others.
    /// Off (the default) sends the raw chunk text to the reranker, byte for byte as before. On sends
    /// each chunk with its context in force ahead of it, so the reranker judges what retrieval matched.
    /// Use [`Config::rerank_input_include_context_on`]; a malformed value reads as off and is logged at boot.
    #[serde(default = "default_switch_off")]
    pub rerank_input_include_context: String,
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
fn default_switch_off() -> String {
    "false".into()
}
fn default_web_rerank_allowed_services() -> String {
    "service:quarry-edge".into()
}
/// Placeholder alert threshold (units per org per UTC day). The owner's figure replaces it;
/// until then it is an alert, never a stop (owner default D-E).
fn default_web_rerank_daily_unit_budget() -> u64 {
    10_000
}
fn default_zero_setting() -> String {
    "0".into()
}
fn default_rerank_provider_tenant_wait_ms() -> String {
    "1000".into()
}
fn default_rerank_breaker_failures() -> String {
    "5".into()
}
fn default_rerank_breaker_window_secs() -> String {
    "30".into()
}
fn default_rerank_breaker_open_secs() -> String {
    "30".into()
}

/// Reads a bounded whole number. Empty counts as the default. Anything that is not a whole number
/// in `min..=max` is an error, which the caller logs; the engine itself does not stop.
pub fn parse_bounded(
    name: &str,
    raw: &str,
    default: u64,
    min: u64,
    max: u64,
) -> anyhow::Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(default);
    }
    match trimmed.parse::<u64>() {
        Ok(value) if (min..=max).contains(&value) => Ok(value),
        _ => anyhow::bail!("{name} must be a whole number from {min} to {max}"),
    }
}

/// Reads a `0`/`1`/`true`/`false` switch. Empty counts as off. Anything else is an error, which
/// the caller logs; the gate then reads as off, never as a silent default.
pub fn parse_switch(name: &str, raw: &str) -> anyhow::Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" | "" => Ok(false),
        _ => anyhow::bail!("{name} must be 0, 1, true or false"),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        // A malformed route switch never stops the engine: it reads as off, and main logs it
        // once at boot. Tenant retrieval does not depend on these gates.
        Ok(envy::from_env::<Config>()?)
    }

    /// Whether `POST /v1/rerank` is switched on. A malformed value reads as off.
    #[must_use]
    pub fn web_rerank_on(&self) -> bool {
        parse_switch("WEB_RERANK_ENABLED", &self.web_rerank_enabled).unwrap_or(false)
    }

    /// Whether the web-rerank processor is approved. A malformed value reads as not approved.
    #[must_use]
    pub fn web_rerank_processor_ok(&self) -> bool {
        parse_switch(
            "WEB_RERANK_PROCESSOR_APPROVED",
            &self.web_rerank_processor_approved,
        )
        .unwrap_or(false)
    }

    /// QB-M — whether the usage ledger is on. A malformed value reads as off.
    #[must_use]
    pub fn usage_emit_on(&self) -> bool {
        parse_switch("USAGE_EMIT_ENABLED", &self.usage_emit_enabled).unwrap_or(false)
    }

    /// CR-T15 — whether the pipeline's rerank sends chunk context. A malformed value reads as off, and
    /// `main` names it once at boot.
    #[must_use]
    pub fn rerank_input_include_context_on(&self) -> bool {
        parse_switch(
            "RERANK_INPUT_INCLUDE_CONTEXT",
            &self.rerank_input_include_context,
        )
        .unwrap_or(false)
    }

    /// QB-T05b — the provider gate's limits. A malformed value is an error for the boot log, and
    /// the caller then uses [`ProviderLimits::default`]: tenant retrieval unbounded and web rerank
    /// without capacity, which is the safe side of every limit.
    pub fn provider_limits(&self) -> anyhow::Result<ProviderLimits> {
        Ok(ProviderLimits {
            max_inflight: parse_bounded(
                "RERANK_PROVIDER_MAX_INFLIGHT",
                &self.rerank_provider_max_inflight,
                0,
                0,
                1_024,
            )? as usize,
            requests_per_second: parse_bounded(
                "RERANK_PROVIDER_RPS",
                &self.rerank_provider_rps,
                0,
                0,
                10_000,
            )? as u32,
            tenant_wait: Duration::from_millis(parse_bounded(
                "RERANK_PROVIDER_TENANT_WAIT_MS",
                &self.rerank_provider_tenant_wait_ms,
                1_000,
                0,
                30_000,
            )?),
            breaker: BreakerPolicy {
                failures: parse_bounded(
                    "RERANK_BREAKER_FAILURES",
                    &self.rerank_breaker_failures,
                    5,
                    1,
                    1_000,
                )? as u32,
                window: Duration::from_secs(parse_bounded(
                    "RERANK_BREAKER_WINDOW_SECS",
                    &self.rerank_breaker_window_secs,
                    30,
                    1,
                    3_600,
                )?),
                open_for: Duration::from_secs(parse_bounded(
                    "RERANK_BREAKER_OPEN_SECS",
                    &self.rerank_breaker_open_secs,
                    30,
                    1,
                    3_600,
                )?),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_route_switches_read_both_spellings_and_refuse_anything_else() {
        assert!(parse_switch("X", "1").expect("1"));
        assert!(parse_switch("X", "true").expect("true"));
        assert!(parse_switch("X", " TRUE ").expect("trimmed and case-folded"));
        assert!(!parse_switch("X", "0").expect("0"));
        assert!(!parse_switch("X", "false").expect("false"));
        assert!(!parse_switch("X", "").expect("empty is off"));
        // A typo is an error, never a silent default.
        assert!(parse_switch("X", "yes").is_err());
        assert!(parse_switch("X", "2").is_err());
    }

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

    /// QB-M: the usage ledger's switch is off unless it reads `1` or `true`, and a typo is off too
    /// (the boot log names it), never a silent on: a paid call is made only when it can be recorded.
    #[test]
    fn the_usage_ledger_switch_is_off_by_default_and_a_typo_is_off() {
        let read = |extra: &[(&str, &str)]| -> bool {
            let mut pairs = vec![
                (
                    "DATABASE_URL".to_string(),
                    "postgres://test.invalid/test".to_string(),
                ),
                (
                    "QDRANT_URL".to_string(),
                    "http://test.invalid:6334".to_string(),
                ),
            ];
            pairs.extend(
                extra
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string())),
            );
            let cfg: Config = envy::from_iter(pairs).expect("config");
            cfg.usage_emit_on()
        };
        assert!(!read(&[]), "off by default");
        assert!(read(&[("USAGE_EMIT_ENABLED", "1")]));
        assert!(read(&[("USAGE_EMIT_ENABLED", "true")]));
        assert!(!read(&[("USAGE_EMIT_ENABLED", "0")]));
        assert!(
            !read(&[("USAGE_EMIT_ENABLED", "yes")]),
            "a typo reads as off"
        );
    }

    /// CR-T15: `RERANK_INPUT_INCLUDE_CONTEXT` is off unless it reads `1` or `true`. A typo is off
    /// too, and the boot check names it: the reranker then judges the raw chunk text, as it always did.
    #[test]
    fn the_rerank_input_switch_is_off_by_default_and_a_typo_is_off() {
        let read = |extra: &[(&str, &str)]| -> bool {
            let mut pairs = vec![
                (
                    "DATABASE_URL".to_string(),
                    "postgres://test.invalid/test".to_string(),
                ),
                (
                    "QDRANT_URL".to_string(),
                    "http://test.invalid:6334".to_string(),
                ),
            ];
            pairs.extend(
                extra
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string())),
            );
            let cfg: Config = envy::from_iter(pairs).expect("config");
            cfg.rerank_input_include_context_on()
        };
        assert!(!read(&[]), "off by default");
        assert!(read(&[("RERANK_INPUT_INCLUDE_CONTEXT", "1")]));
        assert!(read(&[("RERANK_INPUT_INCLUDE_CONTEXT", "true")]));
        assert!(!read(&[("RERANK_INPUT_INCLUDE_CONTEXT", "0")]));
        assert!(
            !read(&[("RERANK_INPUT_INCLUDE_CONTEXT", "yes")]),
            "a typo reads as off"
        );
        assert!(
            parse_switch("RERANK_INPUT_INCLUDE_CONTEXT", "yes").is_err(),
            "the boot check refuses a typo by name"
        );
    }
}
