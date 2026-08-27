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
    /// Auth Core service-token endpoint used to mint an audience-bound
    /// inference-core bearer for each tenant-scoped embedding batch.
    #[serde(default = "default_model_plane_inference_token_url")]
    pub model_plane_inference_token_url: String,
    #[serde(default = "default_model_plane_inference_token_issuer")]
    pub model_plane_inference_token_issuer: String,
    #[serde(default = "default_model_plane_inference_service_id")]
    pub model_plane_inference_service_id: String,
    /// Retention posture auth-core is configured to mint for this service's
    /// `inference-core` audience: `persistent` (zdr:false) or `zdr` (zdr:true).
    /// Must match the principal's `retentionByAudience` entry in
    /// `PLANE_SERVICE_PRINCIPALS_JSON`, or every mint fails closed.
    #[serde(default = "default_model_plane_inference_retention_posture")]
    pub model_plane_inference_retention_posture: String,
    #[serde(default)]
    pub model_plane_inference_service_api_key: String,

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

    // Audio + video arms, served by the self-hosted `media-embedder` sidecar
    // (LAION-CLAP / X-CLIP or SigLIP 2). Both dark until `media_embedder_endpoint` is
    // set, exactly like the visual arm above. Self-hosted out of necessity, not
    // preference: Azure ships no audio-similarity embedder and its video
    // analyzer is extraction rather than embedding — see
    // docs/core-research/embedding-modality-and-rag-audit-2026-08-19.md §3.
    #[serde(default)]
    pub media_embedder_endpoint: String,
    #[serde(default = "default_audio_collection")]
    pub qdrant_audio_collection: String,
    /// LAION-CLAP projection dim. Qdrant fixes vector size per collection, so
    /// changing the tower (e.g. to GLAP) means a new collection, not a resize.
    #[serde(default = "default_audio_dim")]
    pub audio_embedding_dimension: u64,
    #[serde(default = "default_video_collection")]
    pub qdrant_video_collection: String,
    /// Video tower projection dim: X-CLIP base-patch32 = 512 (the default),
    /// SigLIP 2 base/patch16-224 = 768. A tower swap changes the SPACE, not
    /// just the width, so it always means a new collection. Must match what
    /// media-embedder's /healthz reports as `dim`.
    #[serde(default = "default_video_dim")]
    pub video_embedding_dimension: u64,

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
    /// Verifies `dataplane.documents.deleted`, signed by documents-api-go.
    /// Required for the per-document erasure consumer
    /// (`document_erasure_consumer`) — unlike the visual/CAS arm below, this
    /// has no "unconfigured, disabled" state: the vector-purge half of
    /// erasure doesn't depend on CAS being configured, so this key is
    /// required whenever signed event consumers are enabled at all.
    #[serde(default)]
    pub documents_event_public_key_path: String,

    // GDPR/DSAR erasure completeness for the visual arm's MinIO CAS objects
    // (raw page binaries + rendered PNGs). Unset `cas_bucket` → CAS erasure
    // disabled, matching every other optionally-configured arm in this file
    // (e.g. `cohere_embed_v4_endpoint`): the Qdrant-vector half of erasure
    // still runs, only the CAS half no-ops. `cas_endpoint_url` overrides the
    // standard `AWS_ENDPOINT_URL` env var when set; leave empty to fall back
    // to the AWS SDK's normal credential/endpoint resolution.
    #[serde(default)]
    pub cas_bucket: String,
    #[serde(default)]
    pub cas_endpoint_url: String,

    // ── Contextual Retrieval (see `provider::contextualize`) ────────────────
    //
    // Prepends LLM-generated situating context to each chunk before embedding.
    // OFF by default, and the default is load-bearing rather than cautious
    // boilerplate: enabling it spends one inference call per chunk on every
    // first-time embed, which is a real and unbounded per-tenant cost that
    // should be a decision, not a side effect of deploying.
    #[serde(default)]
    pub contextual_retrieval_enabled: bool,
    /// Model id passed to inference-core. No default: a silently-chosen model
    /// would silently choose the cost. Required when the feature is enabled.
    #[serde(default)]
    pub contextual_retrieval_model: String,
    /// Optional provider hint for inference-core's router. Empty = let the
    /// Model Plane route by its own policy, which is the normal case.
    #[serde(default)]
    pub contextual_retrieval_provider_hint: String,
    /// Chars of document text placed in the prompt. Documents are arbitrary
    /// user uploads and can be megabytes; the cap is what stops one oversized
    /// file from either exceeding the model's context window or costing
    /// hundreds of times a normal document. ~48k chars ≈ 12k tokens.
    #[serde(default = "default_contextual_max_document_chars")]
    pub contextual_retrieval_max_document_chars: usize,
    /// Output cap. The prompt asks for one or two sentences; 128 tokens leaves
    /// headroom without inviting a paragraph that would dilute the chunk's own
    /// terms in the embedding.
    #[serde(default = "default_contextual_max_tokens")]
    pub contextual_retrieval_max_tokens: i32,
    #[serde(default = "default_contextual_timeout_ms")]
    pub contextual_retrieval_timeout_ms: u64,
    /// How many chunks of one document are contextualized concurrently. Bounded
    /// so a single large document cannot open a hundred simultaneous inference
    /// streams and crowd out every other tenant's embedding traffic.
    #[serde(default = "default_contextual_concurrency")]
    pub contextual_retrieval_concurrency: usize,
    /// Retry attempts per chunk when inference answers with a transient status
    /// (rate limit, unavailable, timeout).
    ///
    /// Without this, a provider rate limit silently costs coverage: the chunk
    /// falls back to embedding raw, the ingest reports success, and nothing
    /// records that the context is missing. A bulk backfill against an Azure
    /// deployment with a per-minute quota is exactly the case that hits it —
    /// measured 864 of 1,164 chunks (74%) contextualized before this existed,
    /// with the shortfall entirely `ResourceExhausted`.
    ///
    /// 4 attempts with the backoff below spans ~30s per chunk, which covers the
    /// 30-second window Azure's `Retry-After` asks for on this deployment.
    #[serde(default = "default_contextual_retry_attempts")]
    pub contextual_retrieval_retry_attempts: u32,
    /// First backoff step; each subsequent attempt doubles it.
    #[serde(default = "default_contextual_retry_base_ms")]
    pub contextual_retrieval_retry_base_ms: u64,

    // ── Caption-to-text for video (see `provider::video_caption`) ───────────
    //
    // Describes each video segment's filmstrip in ordered prose and indexes that
    // prose as TEXT, which is the only way motion and ordering become
    // retrievable — no video embedding tower distinguishes a video from its
    // reverse (see docs/core-research/video-temporal-retrieval-gap-2026-08-25.md).
    //
    // OFF by default: one vision call per video segment is a real per-tenant
    // cost, and unlike the rest of the media arm this path EGRESSES content, so
    // it must be an explicit decision.
    #[serde(default)]
    pub video_caption_enabled: bool,
    /// Vision model id passed to inference-core. No default, so the cost is
    /// never chosen implicitly. Required when the feature is enabled.
    #[serde(default)]
    pub video_caption_model: String,
    #[serde(default)]
    pub video_caption_provider_hint: String,
    /// Output cap. The prompt asks for two to four sentences; 256 tokens leaves
    /// room for that without inviting an essay that dilutes the retrieval signal.
    #[serde(default = "default_video_caption_max_tokens")]
    pub video_caption_max_tokens: i32,
    #[serde(default = "default_video_caption_timeout_ms")]
    pub video_caption_timeout_ms: u64,
}

fn default_video_caption_max_tokens() -> i32 {
    256
}
fn default_video_caption_timeout_ms() -> u64 {
    60_000
}

fn default_contextual_max_document_chars() -> usize {
    48_000
}
fn default_contextual_max_tokens() -> i32 {
    128
}
fn default_contextual_timeout_ms() -> u64 {
    30_000
}
fn default_contextual_concurrency() -> usize {
    4
}
fn default_contextual_retry_attempts() -> u32 {
    4
}
fn default_contextual_retry_base_ms() -> u64 {
    2_000
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
fn default_model_plane_inference_token_url() -> String {
    "http://auth-core:3011/api/inference-core/internal-token".into()
}
fn default_model_plane_inference_token_issuer() -> String {
    "http://localhost:3011/api/convex-auth".into()
}
/// Matches the deployed registry, which pins `embedding-engine` ->
/// `inference-core` to `persistent`. Note this is the opposite of auth-core's
/// own default for an *unconfigured* audience (`zdr`), so tightening the
/// registry requires setting this variable too.
fn default_model_plane_inference_retention_posture() -> String {
    "persistent".into()
}
fn default_model_plane_inference_service_id() -> String {
    "embedding-engine".into()
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
fn default_audio_collection() -> String {
    "dataplane_audio_segments".into()
}
fn default_audio_dim() -> u64 {
    512
}
fn default_video_collection() -> String {
    "dataplane_video_segments_siglip2".into()
}
fn default_video_dim() -> u64 {
    // SigLIP 2 base/patch16-224 (the default tower) projects to 768.
    // X-CLIP base-patch32 would be 512 — switching towers means switching
    // BOTH this and the collection, which is why the collection names the tower.
    768
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
