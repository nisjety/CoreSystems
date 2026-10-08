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
    /// `inference-core` audience: `persistent` (zdr:false), `zdr` (zdr:true) or
    /// `tenant` (zdr is the ORGANIZATION's own setting, resolved at the mint;
    /// either value is accepted and the bearer carries it, and the token lives
    /// 60 s). Must match the principal's `retentionByAudience` entry in the
    /// principal registry, or every mint fails closed; a `tenant` client works
    /// against every entry, which is why clients ship first and the registry
    /// follows. Read from `MODEL_PLANE_INFERENCE_RETENTION_POSTURE`, which
    /// compose sets from `MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE`
    /// (the graph-index service has its own host variable). An unknown value is
    /// a startup error.
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
    /// The usage ledger (CR-T10): record every paid provider call this service makes
    /// (contextual retrieval, embedding) as a version 2 usage event, signed with this
    /// service's event key and published to the durable `DATAPLANE_COST` stream. Read from
    /// `USAGE_EMIT_ENABLED`; `1` or `true` turns it on.
    ///
    /// OFF by default, and it must stay off until data-orchestrator runs the consumer that
    /// reads the stream (consumers and verifiers ship before producers: an event the old
    /// consumer cannot read is logged as invalid, and dropped by it). Turned on, the service
    /// refuses to start unless signed events are enabled and the stream can be created or
    /// verified: a producer that cannot reach a durable ledger must not run with metering on,
    /// because every event it published would be lost without a sound. While it is off the
    /// version 1 cost event is published as before.
    #[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]
    pub usage_emit_enabled: bool,

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
    // Prepends LLM-generated situating context to each chunk before embedding. ON by default and
    // kept on (owner ruling of 2026-10-03): `CONTEXTUAL_RETRIEVAL_ENABLED=false` is the incident
    // kill switch, accepted only with `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON`, logged at startup and
    // reported by `/readyz` (`Config::contextual_retrieval_state`). Its cost is bounded and metered
    // (inline time boxes, the reconciler's caps and breaker), not decided by leaving it off.
    #[serde(default = "default_true", deserialize_with = "on_unless_off")]
    pub contextual_retrieval_enabled: bool,
    /// Model id passed to inference-core: the Azure deployment that answers. Defaults to
    /// `gpt-4o-mini`, the shared deployment, and naming another is the one change a dedicated
    /// background deployment alias needs (an operator action on inference-core's catalog). A BLANK
    /// value is refused at startup while the feature is on: a model, and so a cost, is never implicit.
    #[serde(default = "default_contextual_model")]
    pub contextual_retrieval_model: String,
    /// Why the incident kill switch is thrown. Required, and checked, when
    /// `CONTEXTUAL_RETRIEVAL_ENABLED=false` (see `Config::contextual_retrieval_state`); ignored
    /// while the feature is on.
    #[serde(default)]
    pub context_retrieval_kill_switch_reason: String,
    /// Provider hint for inference-core's router: which provider serves the
    /// call. Defaults to `azure-openai` (underscore or hyphen both match), the
    /// deployment this feature's data processing is documented for. An empty
    /// value takes the default rather than meaning "any provider": compose
    /// passes every variable, empty when unset, and an unpinned route would let
    /// chunk text go to whichever provider the chain tries next.
    #[serde(default = "default_contextual_provider_hint")]
    pub contextual_retrieval_provider_hint: String,
    /// Residency floor passed to inference-core: it refuses, before any provider
    /// is tried, a route that cannot prove it keeps the call inside it. One of
    /// `global`, `eu` (`eea`), `norway` (`no`, `sovereign`); anything else stops
    /// the service at startup. Defaults to `eu`, and an empty value takes the
    /// default: the weaker floor is spelled out (`global`), never implied by
    /// leaving a variable blank.
    ///
    /// The floor is only as good as inference-core's own proof of where the
    /// deployment is: its `AZURE_OPENAI_REGION` must name an EU region, or the
    /// Azure deployment reads as unproven and every call is refused with
    /// `residency_unavailable` (an outcome of its own, so it is loud).
    #[serde(default = "default_contextual_min_residency")]
    pub contextual_retrieval_min_residency: String,
    /// The prompt version used when an organization has not pinned another:
    /// `ctx-v1` (what has always been stored) or `ctx-v2`. A pin to `ctx-v2` ships
    /// only through the evaluation gate; an unknown value stops the service at
    /// startup.
    #[serde(default = "default_contextual_prompt_version")]
    pub contextual_retrieval_prompt_version: String,
    /// Characters of document text in one prompt. A document up to this size is
    /// read whole; a longer one is read in fixed sections of this size that overlap
    /// by an eighth, each with a header. At least 8,000, and not more than
    /// `contextual_retrieval_max_document_chars`.
    #[serde(default = "default_contextual_window_chars")]
    pub contextual_retrieval_window_chars: usize,
    /// The most document text a prompt may carry, whatever the window is set to.
    /// Documents are arbitrary user uploads and can be megabytes; the cap is what
    /// stops a misconfigured window from either exceeding the model's context
    /// window or costing hundreds of times a normal document. ~48k chars ≈ 12k
    /// tokens.
    #[serde(default = "default_contextual_max_document_chars")]
    pub contextual_retrieval_max_document_chars: usize,
    /// How long one batch may spend generating contexts inline, in milliseconds.
    /// A chunk not finished by then is embedded raw and left `pending` for the
    /// reconciler. `0` generates nothing inline. Must not exceed 30,000: half the
    /// 60 s ack wait of the consumer, because a batch that runs past it is
    /// redelivered and its contexts are paid for twice. Checked at startup.
    #[serde(default = "default_context_inline_budget_ms")]
    pub context_inline_budget_ms: u64,
    /// The same bound for one document. Never past the batch's.
    #[serde(default = "default_context_inline_document_budget_ms")]
    pub context_inline_document_budget_ms: u64,
    /// How many documents of one batch are contextualized at the same time, so one
    /// slow document or one slow tenant cannot hold up the rest of the batch (each
    /// document still bounds its own chunks with
    /// `CONTEXTUAL_RETRIEVAL_CONCURRENCY`, so the most provider calls one batch has in
    /// flight is the product of the two). Between 1 and 32, checked at startup.
    #[serde(default = "default_context_inline_document_concurrency")]
    pub context_inline_document_concurrency: usize,
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

    // ── The context reconciler (CR-T9a, see `context_reconciler`) ───────────
    //
    // The DB-truth loop that backfills, repairs and verifies contexts and vectors: it fills
    // `pending` and `failed_transient` units, verifies the `legacy` ones (re-embedding the
    // stored context, no model call), re-embeds a vector that does not match its stored
    // context, re-checks zero-retention skips once an hour and, only when asked, refreshes
    // contexts made with a superseded prompt. Every number below is checked at startup
    // (`context_reconciler::Settings::from_config`); a value out of range stops the service
    // and says which variable.
    /// `CONTEXT_RECONCILER_ENABLED`. OFF by default and it stays off until the legacy
    /// verification and the first capped run have been proven on the live stack (the runbook's
    /// section 5.2). Turned on, the service refuses to start unless signed events are enabled:
    /// a healed document is announced with `documents.indexed`, which the keyword index only
    /// reads signed.
    #[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]
    pub context_reconciler_enabled: bool,
    /// Seconds between passes when there was nothing to do (a pass that did work is followed
    /// by the next at once, after a short jittered pause). 1 to 3,600; jittered by up to 20 %.
    #[serde(default = "default_reconciler_interval_secs")]
    pub context_reconciler_interval_secs: u64,
    /// The most units one pass leases, across every organization. 1 to 500.
    #[serde(default = "default_reconciler_units_per_pass")]
    pub context_reconciler_units_per_pass: usize,
    /// The most units one pass leases for one organization, so that one organization's backlog
    /// does not starve the others. 1 to `CONTEXT_RECONCILER_UNITS_PER_PASS`.
    #[serde(default = "default_reconciler_org_units_per_pass")]
    pub context_reconciler_org_units_per_pass: usize,
    /// Documents worked on at the same time, across every organization. 1 to 16.
    #[serde(default = "default_reconciler_concurrency")]
    pub context_reconciler_concurrency: usize,
    /// Documents of one organization worked on at the same time. 1 to the global figure.
    #[serde(default = "default_reconciler_org_concurrency")]
    pub context_reconciler_org_concurrency: usize,
    /// How many chunks of one document are contextualized at once by the reconciler (the inline
    /// path has its own, `CONTEXTUAL_RETRIEVAL_CONCURRENCY`). 1 to 16.
    #[serde(default = "default_reconciler_chunk_concurrency")]
    pub context_reconciler_chunk_concurrency: usize,
    /// How long a leased unit belongs to the reconciler that leased it, in seconds. A crashed
    /// reconciler's units are free again when it passes. 60 to 3,600, and more than twice the
    /// time box below.
    #[serde(default = "default_reconciler_lease_secs")]
    pub context_reconciler_lease_secs: u64,
    /// The time box of one batch the reconciler runs, in milliseconds, up to 30,000 (the bound
    /// every batch has; see `CONTEXT_INLINE_BUDGET_MS`). The reconciler has no broker waiting on
    /// it, so it takes the whole bound by default and finishes a document in one pass.
    #[serde(default = "default_reconciler_budget_ms")]
    pub context_reconciler_budget_ms: u64,
    /// The same for one document. Never more than the batch's.
    #[serde(default = "default_reconciler_document_budget_ms")]
    pub context_reconciler_document_budget_ms: u64,
    /// Failed attempts after which a `failed_transient` unit becomes `failed_permanent`, with an
    /// alert. A posture or credential problem never counts as an attempt. 1 to 100.
    #[serde(default = "default_reconciler_max_attempts")]
    pub context_reconciler_max_attempts: i32,
    /// Consecutive passes in which the provider failed everything before the circuit breaker
    /// pauses the reconciler. 1 to 20.
    #[serde(default = "default_reconciler_breaker_failures")]
    pub context_reconciler_breaker_failures: u32,
    /// How long the breaker keeps the reconciler paused, in seconds. 10 to 3,600. A refused
    /// credential opens it at once, for the same time.
    #[serde(default = "default_reconciler_breaker_pause_secs")]
    pub context_reconciler_breaker_pause_secs: u64,
    /// A file whose existence pauses the reconciler, checked at the start of every pass:
    /// the kill switch that needs no restart (`touch` it inside the container to pause, remove
    /// it to resume). Empty disables it.
    #[serde(default = "default_reconciler_pause_file")]
    pub context_reconciler_pause_file: String,
    /// Seconds between two checks of an organization whose units were skipped for zero
    /// retention (one bearer mint, which says whether the organization is still zero-retention).
    /// 60 to 86,400; the default is an hour.
    #[serde(default = "default_reconciler_zdr_recheck_secs")]
    pub context_reconciler_zdr_recheck_secs: u64,
    /// Seconds between the sweeps that look for the work the work queue index does not cover
    /// (vectors that do not match their stored context, units skipped by a policy that was lifted,
    /// stale contexts). 10 to 86,400.
    #[serde(default = "default_reconciler_sweep_secs")]
    pub context_reconciler_sweep_secs: u64,
    /// `CONTEXT_RECONCILER_STALE_ENABLED`: regenerate the contexts of a document that changed
    /// after they were made (candidates are found by `documents.updated_at` against
    /// `context_generated_at`, then each chunk's input hash decides, so an unchanged chunk costs
    /// nothing). OFF by default: an edited document costs one provider call per chunk whose
    /// window changed, and that is a decision.
    #[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]
    pub context_reconciler_stale_enabled: bool,
    /// Documents one pass checks for staleness. 1 to 100.
    #[serde(default = "default_reconciler_stale_documents")]
    pub context_reconciler_stale_documents: usize,
    /// `CONTEXT_REFRESH_ON_PROMPT_CHANGE`: regenerate the stored contexts made with a prompt
    /// version other than the one in force for their organization. OFF by default, and a refresh
    /// is always capped: turned on it requires `CONTEXT_REFRESH_MAX_UNITS` above zero.
    #[serde(default, deserialize_with = "usage_ledger_rs::deserialize_flag")]
    pub context_refresh_on_prompt_change: bool,
    /// The most units one run of the process refreshes, in all (a restart grants the cap again).
    #[serde(default)]
    pub context_refresh_max_units: u64,

    // ── The background LLM budget (CR-T9b, see `budget`) ────────────────────
    //
    // Owner default D-E: configurable per organization (`org_processing_policy`) and here for
    // every organization that sets none; ALERT-ONLY by default (exceeding a budget alerts and
    // keeps working), `stop` only when an operator says so, and NO figure and NO prices built in
    // (the owner supplies them). graph-index reads the same four variables, from the same compose
    // interpolation, so one setting holds for both. Strings, so that blank is unset (compose
    // passes `${VAR:-}`); checked at startup by `Config::background_budget`.
    /// `BACKGROUND_BUDGET_MODE`: `alert_only` (blank, the default), `throttle` or `stop`: what an
    /// organization that does not set its own `budget_mode` gets.
    #[serde(default)]
    pub background_budget_mode: String,
    /// `BACKGROUND_BUDGET_MONTHLY_USD`: the figure an organization without one of its own is held
    /// to. Blank means none: with no figure there is nothing to exceed.
    #[serde(default)]
    pub background_budget_monthly_usd: String,
    /// `BACKGROUND_PRICE_INPUT_USD_PER_MTOK` and `BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK`: what a
    /// million tokens cost, which turns the usage ledger's tokens into the dollars a budget is
    /// stated in. Both or neither; never defaulted.
    #[serde(default)]
    pub background_price_input_usd_per_mtok: String,
    #[serde(default)]
    pub background_price_output_usd_per_mtok: String,

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

fn default_true() -> bool {
    true
}
/// The shared Azure deployment. A dedicated background deployment alias is another value of the
/// same variable, so both are supported and the shared one is the default until the alias exists.
fn default_contextual_model() -> String {
    "gpt-4o-mini".into()
}
fn default_contextual_provider_hint() -> String {
    "azure-openai".into()
}

/// A switch that is ON unless it is deliberately turned off: `true` or `1` is on, `false` or `0`
/// is off, and an EMPTY value is the default, which is on, never off. Anything else stops the
/// service at startup and names the variable: a typo (`flase`, `off`, `no`) must neither leave
/// the incident kill switch thrown nor silently leave it unthrown. (Its sibling
/// `usage_ledger_rs::deserialize_flag` reads an empty value as off, because the switches that use
/// it are off by default; the two defaults point opposite ways and so must the two readers.)
fn on_unless_off<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = <String as Deserialize>::deserialize(deserializer)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        other => Err(<D::Error as serde::de::Error>::custom(format!(
            "CONTEXTUAL_RETRIEVAL_ENABLED must be true or 1 (on), false or 0 (the incident kill \
             switch, which needs CONTEXT_RETRIEVAL_KILL_SWITCH_REASON) or empty (the default, on); \
             got `{other}`"
        ))),
    }
}

/// The shortest reason the incident kill switch accepts, in characters. Short enough for "provider
/// incident", long enough to refuse `x`, `none` and `todo`.
pub const KILL_SWITCH_REASON_MIN_CHARS: usize = 8;
/// The longest, in characters. The reason is logged at startup and reported by `/readyz`, so it
/// is bounded, one line, and written to be read by an operator.
pub const KILL_SWITCH_REASON_MAX_CHARS: usize = 240;

/// What the global contextual retrieval flag says once the boot-time contract has been applied:
/// on, or the incident kill switch with the reason the operator gave. This is the one value that
/// `main` logs at startup and `/readyz` reports.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContextualRetrievalState {
    /// Contexts are generated, with this model.
    On { model: String },
    /// The incident kill switch is thrown: no model is asked and chunks are embedded raw, for
    /// this reason. Nothing stored is removed.
    KillSwitch { reason: String },
}

impl ContextualRetrievalState {
    pub fn is_on(&self) -> bool {
        matches!(self, Self::On { .. })
    }

    /// The stable word `/readyz` and the logs use.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::On { .. } => "on",
            Self::KillSwitch { .. } => "kill_switch",
        }
    }
}
fn default_contextual_min_residency() -> String {
    "eu".into()
}
fn default_contextual_prompt_version() -> String {
    "ctx-v1".into()
}
fn default_contextual_window_chars() -> usize {
    32_000
}
fn default_context_inline_budget_ms() -> u64 {
    20_000
}
fn default_context_inline_document_budget_ms() -> u64 {
    8_000
}
/// Two documents at a time, so one slow document or tenant no longer holds the batch, and
/// the most one batch has in flight (two times `CONTEXTUAL_RETRIEVAL_CONCURRENCY`, four) is
/// double what the sequential loop ever asked of a deployment that background work shares
/// with chat. Raise it with the dedicated deployment alias, not before.
fn default_context_inline_document_concurrency() -> usize {
    2
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

// The reconciler's defaults are chosen for a deployment that background work shares with
// chat: one pass leases at most 32 units, two documents at a time, two chunks of each at a
// time, so the most it has in flight at the provider is four calls (the inline path's own
// default is the same four). Raise them with the dedicated deployment alias, not before.
fn default_reconciler_interval_secs() -> u64 {
    30
}
fn default_reconciler_units_per_pass() -> usize {
    32
}
fn default_reconciler_org_units_per_pass() -> usize {
    16
}
fn default_reconciler_concurrency() -> usize {
    2
}
fn default_reconciler_org_concurrency() -> usize {
    1
}
fn default_reconciler_chunk_concurrency() -> usize {
    2
}
fn default_reconciler_lease_secs() -> u64 {
    300
}
fn default_reconciler_budget_ms() -> u64 {
    30_000
}
fn default_reconciler_document_budget_ms() -> u64 {
    25_000
}
fn default_reconciler_max_attempts() -> i32 {
    8
}
fn default_reconciler_breaker_failures() -> u32 {
    3
}
fn default_reconciler_breaker_pause_secs() -> u64 {
    300
}
fn default_reconciler_pause_file() -> String {
    "/tmp/context-reconciler.pause".into()
}
fn default_reconciler_zdr_recheck_secs() -> u64 {
    3_600
}
fn default_reconciler_sweep_secs() -> u64 {
    300
}
fn default_reconciler_stale_documents() -> usize {
    8
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
/// registry requires setting this variable too. The enablement step sets it to
/// `tenant` after the Control Plane image has the tenant code (see the
/// contextual retrieval runbook); `tenant` also works against the `persistent`
/// entry, so nothing breaks if the variable is set first.
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
    /// The configuration of the process, with the boot-time contract of contextual retrieval
    /// applied: every caller (the service, the operator's `context-backfill`) refuses an
    /// unexplained kill switch at the same place.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::checked(envy::from_env::<Config>()?)
    }

    fn checked(self) -> anyhow::Result<Self> {
        self.contextual_retrieval_state()?;
        self.background_budget()?;
        Ok(self)
    }

    /// The deployment-wide background LLM budget (CR-T9b), checked at startup by every caller
    /// of [`Config::from_env`]: the service and the operator's `context-backfill` refuse a
    /// setting that cannot work, naming the variable.
    ///
    /// * An unknown mode, a figure or price that is not a number from 0 up, only one of the two
    ///   prices, or a `throttle` or `stop` with a figure and no prices (a limit that could never
    ///   fire) are refused (`GlobalBudget::parse`).
    /// * So is a `throttle` or `stop` with a figure while the usage ledger is off
    ///   (`USAGE_EMIT_ENABLED`): this service's own calls would not be recorded, the month's
    ///   spend would be a part of the truth, and the limit would fire late or never. Alert only
    ///   is allowed in that state: it never stops anything, and it says in the log that it
    ///   cannot measure.
    ///
    /// Nothing about the default (alert only, no figure, no prices) can fail.
    pub(crate) fn background_budget(&self) -> anyhow::Result<crate::budget::GlobalBudget> {
        let budget = crate::budget::GlobalBudget::parse(
            &self.background_budget_mode,
            &self.background_budget_monthly_usd,
            &self.background_price_input_usd_per_mtok,
            &self.background_price_output_usd_per_mtok,
        )
        .map_err(|message| anyhow::anyhow!(message))?;
        if budget.acts_on_spend() && !self.usage_emit_enabled {
            anyhow::bail!(
                "BACKGROUND_BUDGET_MODE={} with BACKGROUND_BUDGET_MONTHLY_USD needs \
                 USAGE_EMIT_ENABLED=1: the budget is measured from the usage ledger, and while \
                 it is off this service's own calls are not in it, so the limit would fire late \
                 or never. Turn the ledger on first (alert_only needs nothing)",
                budget.mode.as_str()
            );
        }
        Ok(budget)
    }

    /// The boot-time contract of the global contextual retrieval flag (CR-T13).
    ///
    /// * On (the default): a model must be named. Blank is refused, never read as "no model".
    /// * Off: this is the incident kill switch, and a switch that is off must say why. The reason
    ///   (`CONTEXT_RETRIEVAL_KILL_SWITCH_REASON`) is trimmed and must be 8 to 240 characters on
    ///   one line, with no control character. Contextual retrieval stays on (owner ruling of
    ///   2026-10-03): the switch is for an incident in which content must stop going to the
    ///   provider, never for load or cost, so an off that nobody explained is refused at startup
    ///   rather than left off by accident. The reason is logged at startup and reported by
    ///   `/readyz`, so it must be safe for an operator to read: no customer name, no personal data.
    ///
    /// A reason supplied while the feature is on is ignored (a leftover of a past incident must
    /// not stop a service from starting).
    pub fn contextual_retrieval_state(&self) -> anyhow::Result<ContextualRetrievalState> {
        if self.contextual_retrieval_enabled {
            let model = self.contextual_retrieval_model.trim();
            if model.is_empty() {
                anyhow::bail!(
                    "CONTEXTUAL_RETRIEVAL_MODEL is required when CONTEXTUAL_RETRIEVAL_ENABLED=true: \
                     unset it for the default (gpt-4o-mini), or name the deployment; a blank value \
                     would leave the model, and so the cost, implicit"
                );
            }
            return Ok(ContextualRetrievalState::On {
                model: model.to_owned(),
            });
        }
        let reason = self.context_retrieval_kill_switch_reason.trim();
        if reason.is_empty() {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_ENABLED=false is the incident kill switch and needs \
                 CONTEXT_RETRIEVAL_KILL_SWITCH_REASON: say why content must stop going to the \
                 provider ({KILL_SWITCH_REASON_MIN_CHARS} to {KILL_SWITCH_REASON_MAX_CHARS} \
                 characters, one line). Contextual retrieval stays on (owner ruling of \
                 2026-10-03), so a switch that is off without a reason is refused: turning it off \
                 must be a deliberate, recorded act, never an accident and never a way to relieve \
                 load"
            );
        }
        let length = reason.chars().count();
        if length < KILL_SWITCH_REASON_MIN_CHARS {
            anyhow::bail!(
                "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON is {length} characters; the kill switch \
                 needs a reason of at least {KILL_SWITCH_REASON_MIN_CHARS}, an explanation an \
                 operator can act on and not a placeholder"
            );
        }
        if length > KILL_SWITCH_REASON_MAX_CHARS {
            anyhow::bail!(
                "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON is {length} characters; it is logged at \
                 startup and reported by /readyz, so it may be at most \
                 {KILL_SWITCH_REASON_MAX_CHARS}"
            );
        }
        if reason.chars().any(char::is_control) {
            anyhow::bail!(
                "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON must be one line with no control \
                 character: it is logged at startup and reported by /readyz"
            );
        }
        Ok(ContextualRetrievalState::KillSwitch {
            reason: reason.to_owned(),
        })
    }
}

#[cfg(test)]
mod usage_ledger_config_tests {
    use super::Config;

    fn from(extra: &[(&str, &str)]) -> Config {
        let mut vars = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        vars.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        envy::from_iter(vars).expect("config")
    }

    /// The usage ledger (CR-T10) is off unless the switch is turned on, and `1` turns it on (the
    /// compose default is `0`, so both spellings of a boolean have to read right).
    #[test]
    fn the_usage_ledger_is_off_by_default_and_reads_one_and_true() {
        assert!(!from(&[]).usage_emit_enabled);
        for (value, expected) in [("1", true), ("true", true), ("0", false), ("false", false)] {
            assert_eq!(
                from(&[("USAGE_EMIT_ENABLED", value)]).usage_emit_enabled,
                expected,
                "USAGE_EMIT_ENABLED={value}"
            );
        }
    }
}

/// The defaults of contextual retrieval (CR-T13). It is on, and it takes a model, a provider and a
/// residency floor from the configuration with nothing set: a deployment whose environment says
/// nothing about it generates contexts, which is what the owner ruled (2026-10-03).
#[cfg(test)]
mod contextual_retrieval_default_tests {
    use super::Config;

    fn from(extra: &[(&str, &str)]) -> Config {
        let mut vars = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        vars.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        envy::from_iter(vars).expect("config")
    }

    #[test]
    fn contextual_retrieval_is_on_by_default() {
        assert!(
            from(&[]).contextual_retrieval_enabled,
            "contextual retrieval stays on (owner ruling of 2026-10-03): the flag is the incident kill switch"
        );
    }

    #[test]
    fn the_default_model_route_and_floor_need_no_variable() {
        let cfg = from(&[]);
        assert_eq!(cfg.contextual_retrieval_model, "gpt-4o-mini");
        assert_eq!(cfg.contextual_retrieval_provider_hint, "azure-openai");
        assert_eq!(cfg.contextual_retrieval_min_residency, "eu");
        assert_eq!(cfg.contextual_retrieval_prompt_version, "ctx-v1");
    }
}

/// The boot-time contract of the global flag (CR-T13): contextual retrieval is on, the flag is the
/// incident kill switch, and a kill switch that is thrown must say why. Every case parses the
/// variables the way the service does (through `envy`, the same deserializers) and applies the
/// same check `Config::from_env` applies.
#[cfg(test)]
mod contextual_retrieval_switch_tests {
    use super::{
        Config, ContextualRetrievalState, KILL_SWITCH_REASON_MAX_CHARS,
        KILL_SWITCH_REASON_MIN_CHARS,
    };

    fn vars(extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut vars = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        vars.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        vars
    }

    fn parse(extra: &[(&str, &str)]) -> Result<Config, envy::Error> {
        envy::from_iter::<_, Config>(vars(extra))
    }

    fn state(extra: &[(&str, &str)]) -> anyhow::Result<ContextualRetrievalState> {
        parse(extra)
            .expect("the variables parse")
            .contextual_retrieval_state()
    }

    fn off(reason: &str) -> anyhow::Result<ContextualRetrievalState> {
        state(&[
            ("CONTEXTUAL_RETRIEVAL_ENABLED", "false"),
            ("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON", reason),
        ])
    }

    fn message(result: anyhow::Result<ContextualRetrievalState>) -> String {
        result.expect_err("a refusal").to_string()
    }

    #[test]
    fn a_configuration_that_says_nothing_is_on_with_the_default_model() {
        assert_eq!(
            state(&[]).expect("the defaults are a valid configuration"),
            ContextualRetrievalState::On {
                model: "gpt-4o-mini".to_owned()
            }
        );
    }

    /// Both spellings of a boolean, in either case, and an EMPTY value that means the default.
    /// Empty is on: a variable that is blank (a compose file passes every variable, and an
    /// `.env` line can be left empty) must never be the way the feature is switched off.
    #[test]
    fn the_flag_reads_both_spellings_and_an_empty_value_is_on() {
        for (value, expected) in [
            ("true", true),
            ("TRUE", true),
            (" true ", true),
            ("1", true),
            ("", true),
            ("   ", true),
            ("false", false),
            ("False", false),
            (" false ", false),
            ("0", false),
        ] {
            let cfg = parse(&[("CONTEXTUAL_RETRIEVAL_ENABLED", value)]).expect(value);
            assert_eq!(
                cfg.contextual_retrieval_enabled, expected,
                "CONTEXTUAL_RETRIEVAL_ENABLED={value:?}"
            );
        }
    }

    /// A typo is a startup error that names the variable, not a switch left thrown or unthrown.
    #[test]
    fn a_value_that_is_not_a_boolean_stops_the_service_and_names_the_variable() {
        for value in ["flase", "off", "no", "yes", "on", "2", "disabled", "kill"] {
            let error = parse(&[("CONTEXTUAL_RETRIEVAL_ENABLED", value)])
                .expect_err(value)
                .to_string();
            assert!(
                error.contains("CONTEXTUAL_RETRIEVAL_ENABLED"),
                "{value}: {error}"
            );
        }
    }

    #[test]
    fn a_named_model_is_the_model_that_is_reported() {
        // A dedicated background deployment alias is another value of the same variable.
        assert_eq!(
            state(&[("CONTEXTUAL_RETRIEVAL_MODEL", "  background-gpt-4o-mini  ")])
                .expect("a named model"),
            ContextualRetrievalState::On {
                model: "background-gpt-4o-mini".to_owned()
            }
        );
    }

    #[test]
    fn a_blank_model_is_refused_while_the_feature_is_on() {
        for blank in ["", "   "] {
            let text = message(state(&[("CONTEXTUAL_RETRIEVAL_MODEL", blank)]));
            assert!(text.contains("CONTEXTUAL_RETRIEVAL_MODEL"), "{text}");
        }
    }

    /// The switch needs no model: nothing is asked.
    #[test]
    fn a_blank_model_does_not_matter_while_the_kill_switch_is_thrown() {
        let result = state(&[
            ("CONTEXTUAL_RETRIEVAL_ENABLED", "false"),
            ("CONTEXTUAL_RETRIEVAL_MODEL", ""),
            (
                "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON",
                "provider incident 2026-10-07",
            ),
        ]);
        assert!(matches!(
            result,
            Ok(ContextualRetrievalState::KillSwitch { .. })
        ));
    }

    #[test]
    fn the_kill_switch_without_a_reason_is_refused_and_says_what_to_set() {
        for reason in ["", "   ", "\t \n"] {
            let text = message(off(reason));
            assert!(
                text.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
                "{reason:?}: {text}"
            );
            assert!(
                text.contains("CONTEXTUAL_RETRIEVAL_ENABLED"),
                "{reason:?}: {text}"
            );
        }
        // And with the variable not set at all.
        let text = message(state(&[("CONTEXTUAL_RETRIEVAL_ENABLED", "false")]));
        assert!(
            text.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
            "{text}"
        );
    }

    #[test]
    fn a_placeholder_reason_is_refused() {
        for reason in ["x", "none", "todo", "off", "seven.."] {
            assert!(reason.chars().count() < KILL_SWITCH_REASON_MIN_CHARS);
            let text = message(off(reason));
            assert!(
                text.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
                "{text}"
            );
        }
    }

    #[test]
    fn the_reason_is_bounded_in_characters_not_bytes() {
        // Eight characters, sixteen bytes: long enough.
        let shortest: String = "æøåæøåæø".to_owned();
        assert_eq!(shortest.chars().count(), KILL_SWITCH_REASON_MIN_CHARS);
        assert!(off(&shortest).is_ok());
        let longest = "a".repeat(KILL_SWITCH_REASON_MAX_CHARS);
        assert!(off(&longest).is_ok());
        let text = message(off(&"a".repeat(KILL_SWITCH_REASON_MAX_CHARS + 1)));
        assert!(
            text.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
            "{text}"
        );
        // 240 two-byte characters are 480 bytes and still within the bound.
        assert!(off(&"å".repeat(KILL_SWITCH_REASON_MAX_CHARS)).is_ok());
    }

    /// The reason is logged and reported on one line: a newline would let one value write what
    /// looks like another log line.
    #[test]
    fn a_reason_with_a_control_character_is_refused() {
        for reason in [
            "provider incident\nlevel=INFO forged line",
            "provider\tincident 2026",
            "provider incident \u{7} bell",
            "provider incident \u{1b}[31m",
        ] {
            let text = message(off(reason));
            assert!(
                text.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_reason_is_trimmed_and_reported_as_given() {
        assert_eq!(
            off("  provider incident 2026-10-07: restricted text seen  ").expect("a reason"),
            ContextualRetrievalState::KillSwitch {
                reason: "provider incident 2026-10-07: restricted text seen".to_owned()
            }
        );
    }

    /// A leftover reason from a past incident must not stop the service from starting.
    #[test]
    fn a_reason_is_ignored_while_the_feature_is_on() {
        for reason in ["provider incident 2026-10-07", "x", ""] {
            assert_eq!(
                state(&[("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON", reason)]).expect("on"),
                ContextualRetrievalState::On {
                    model: "gpt-4o-mini".to_owned()
                }
            );
        }
    }

    /// What `from_env` applies: an unexplained switch makes the configuration an error, so no
    /// caller (the service, the operator's tool) gets a configuration it was not allowed to have.
    #[test]
    fn an_unexplained_kill_switch_is_not_a_configuration() {
        let cfg = parse(&[("CONTEXTUAL_RETRIEVAL_ENABLED", "false")]).expect("it parses");
        let error = cfg.checked().expect_err("it is refused").to_string();
        assert!(
            error.contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
            "{error}"
        );
        let cfg = parse(&[
            ("CONTEXTUAL_RETRIEVAL_ENABLED", "false"),
            (
                "CONTEXT_RETRIEVAL_KILL_SWITCH_REASON",
                "provider incident 2026-10-07",
            ),
        ])
        .expect("it parses");
        assert!(cfg.checked().is_ok());
        assert!(parse(&[]).expect("it parses").checked().is_ok());
    }

    /// What `/readyz` and the startup log say, word for word.
    #[test]
    fn the_state_serializes_as_the_readiness_report_reads_it() {
        let on = state(&[]).expect("on");
        assert!(on.is_on());
        assert_eq!(on.as_str(), "on");
        assert_eq!(
            serde_json::to_value(&on).expect("json"),
            serde_json::json!({"state": "on", "model": "gpt-4o-mini"})
        );
        let off = off("provider incident 2026-10-07").expect("kill switch");
        assert!(!off.is_on());
        assert_eq!(off.as_str(), "kill_switch");
        assert_eq!(
            serde_json::to_value(&off).expect("json"),
            serde_json::json!({"state": "kill_switch", "reason": "provider incident 2026-10-07"})
        );
    }
}

/// The deployment-wide background LLM budget (CR-T9b): its defaults and its boot-time check.
#[cfg(test)]
mod background_budget_tests {
    use super::*;
    use crate::budget::{BudgetMode, GlobalBudget};

    fn parse(extra: &[(&str, &str)]) -> Config {
        let mut vars = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        vars.extend(
            extra
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        );
        envy::from_iter(vars).expect("config")
    }

    fn budget(extra: &[(&str, &str)]) -> anyhow::Result<GlobalBudget> {
        parse(extra).background_budget()
    }

    /// Owner default D-E: alert-only, no figure, no prices. Nothing is built in, and nothing about
    /// the default can fail the boot.
    #[test]
    fn the_default_is_alert_only_with_no_figure_and_no_prices_and_always_boots() {
        let default = budget(&[]).expect("the default boots");
        assert_eq!(default, GlobalBudget::default());
        assert_eq!(default.mode, BudgetMode::AlertOnly);
        assert_eq!(default.monthly_usd, None);
        assert_eq!(default.price, None);
        assert!(!default.acts_on_spend());
        // What compose passes when nothing is set: blank for the three that have no default and
        // the documented default for the mode.
        let compose = budget(&[
            ("BACKGROUND_BUDGET_MODE", "alert_only"),
            ("BACKGROUND_BUDGET_MONTHLY_USD", ""),
            ("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", ""),
            ("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", ""),
        ])
        .expect("what compose passes boots");
        assert_eq!(compose, GlobalBudget::default());
        // The whole configuration of the process boots with it.
        assert!(parse(&[]).checked().is_ok());
    }

    #[test]
    fn a_setting_is_read_and_a_bad_one_stops_the_service_naming_the_variable() {
        let set = budget(&[
            ("BACKGROUND_BUDGET_MODE", "alert_only"),
            ("BACKGROUND_BUDGET_MONTHLY_USD", "25"),
            ("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", "0.15"),
            ("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", "0.6"),
        ])
        .expect("an alert-only budget with prices");
        assert_eq!(set.monthly_usd, Some(25.0));
        for (vars, name) in [
            (
                vec![("BACKGROUND_BUDGET_MODE", "sometimes")],
                "BACKGROUND_BUDGET_MODE",
            ),
            (
                vec![("BACKGROUND_BUDGET_MONTHLY_USD", "-3")],
                "BACKGROUND_BUDGET_MONTHLY_USD",
            ),
            (
                vec![("BACKGROUND_BUDGET_MONTHLY_USD", "lots")],
                "BACKGROUND_BUDGET_MONTHLY_USD",
            ),
            (
                vec![("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", "0.15")],
                "set together or not at all",
            ),
            (
                vec![("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", "0.6")],
                "set together or not at all",
            ),
        ] {
            let error = budget(&vars).expect_err("refused").to_string();
            assert!(error.contains(name), "{error}");
            // And the whole configuration refuses it, which is what the service and the
            // operator's tool call.
            assert!(parse(&vars).checked().is_err(), "{vars:?}");
        }
    }

    /// A limit that could never fire is refused at boot rather than believed: it would be a
    /// stop an operator thinks is in force.
    #[test]
    fn a_stop_or_a_throttle_with_a_figure_needs_prices_and_the_usage_ledger() {
        let prices = [
            ("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", "0.15"),
            ("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", "0.6"),
        ];
        for mode in ["stop", "throttle"] {
            let limit = [
                ("BACKGROUND_BUDGET_MODE", mode),
                ("BACKGROUND_BUDGET_MONTHLY_USD", "25"),
            ];
            // No prices: the spend cannot be measured.
            let error = budget(&limit).expect_err("no prices").to_string();
            assert!(
                error.contains("BACKGROUND_PRICE_INPUT_USD_PER_MTOK"),
                "{error}"
            );
            // Prices but no usage ledger: this service's calls are not recorded.
            let with_prices: Vec<(&str, &str)> = limit.iter().chain(&prices).copied().collect();
            let error = budget(&with_prices).expect_err("no ledger").to_string();
            assert!(error.contains("USAGE_EMIT_ENABLED=1"), "{error}");
            assert!(error.contains(mode), "{error}");
            // Both in place: it boots, and it acts on spend.
            let mut complete = with_prices.clone();
            complete.push(("USAGE_EMIT_ENABLED", "1"));
            let ok = budget(&complete).expect("a limit that can fire boots");
            assert!(ok.acts_on_spend());
            assert!(parse(&complete).checked().is_ok());
            // A mode with no figure has nothing to exceed, so it boots without either.
            assert!(budget(&[("BACKGROUND_BUDGET_MODE", mode)]).is_ok());
        }
        // Alert only needs neither: it never stops anything.
        assert!(budget(&[
            ("BACKGROUND_BUDGET_MODE", "alert_only"),
            ("BACKGROUND_BUDGET_MONTHLY_USD", "25"),
        ])
        .is_ok());
    }
}
