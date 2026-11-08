use std::time::Duration;

use quarry_runtime::rerank::DEFAULT_RERANK_DEADLINE;
use serde::Deserialize;

/// The smallest and the largest rerank budget the Data Plane accepts as `deadline_ms`.
const RERANK_DEADLINE_MIN: Duration = Duration::from_millis(100);
const RERANK_DEADLINE_MAX: Duration = Duration::from_millis(8_000);

#[derive(Debug, Deserialize, Clone)]
pub struct EdgeConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_ua")]
    pub user_agent: String,
    #[serde(default = "default_timeout")]
    pub fetch_timeout_s: u64,
    #[serde(default = "default_control_url")]
    pub control_base_url: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub dragonfly_url: Option<String>,
    #[serde(default = "default_artifact_backend")]
    pub artifact_backend: String,
    #[serde(default = "default_artifact_root")]
    pub artifact_root: String,
    #[serde(default)]
    pub s3_bucket: Option<String>,
    #[serde(default)]
    pub control_api_key: String,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
    /// Optional file-backed URL security snapshot. The feed sync script writes
    /// URLhaus/PhishTank host blocklists in quarry-security's native JSON shape.
    #[serde(default)]
    pub security_snapshot_path: Option<String>,
    #[serde(default)]
    pub data_plane_url: Option<String>,
    /// Dedicated Data Plane *ingest* (durable write) target. The retrieval
    /// engine (`data_plane_url`) serves reads/vector-similarity but has no
    /// `POST /v1/documents` write route — that route lives on the Go
    /// documents-api. When set, the ingest client (HTTP or gRPC) posts here;
    /// otherwise it falls back to `data_plane_url` for backward compatibility.
    #[serde(default)]
    pub data_plane_ingest_url: Option<String>,
    #[serde(default)]
    /// Development-only static Data Plane bearer. Production mints per-org
    /// tokens from Auth Core using `quarry_service_api_key`.
    pub data_plane_service_token: Option<String>,
    /// Kill switch for the Data Plane vector leg (`QUARRY_EDGE__VECTOR_RETRIEVAL`): the
    /// `POST /v1/retrieve` call behind hybrid search and find-similar. Off by default, so
    /// search is lexical-only and find-similar answers 501 until the operator turns it on.
    /// Turn it on only after the Data Plane has stamped and backfilled the quarry corpus
    /// (QB-M1b). Needs DATA_PLANE_URL and a service identity.
    #[serde(default)]
    pub vector_retrieval: bool,
    /// Vector calls one org may spend per hour (`QUARRY_EDGE__VECTOR_RETRIEVAL_PER_ORG_HOURLY_BUDGET`).
    /// Each call is one paid query embedding. The count lives in each process, so every replica
    /// keeps its own; metering through the usage ledger replaces it (DEF-QB-VECTOR-METERING).
    #[serde(default = "default_vector_retrieval_budget")]
    pub vector_retrieval_per_org_hourly_budget: u32,
    /// Auth Core base URL used for service-principal token minting.
    #[serde(default = "default_auth_core_url")]
    pub auth_core_url: String,
    /// Durable service-principal credential registered under the fixed
    /// `quarry-edge` identity in Auth Core.
    #[serde(default)]
    pub quarry_service_api_key: Option<String>,
    /// Explicit local-development escape hatch for legacy static plane tokens.
    /// Refused when ENVIRONMENT is prod/production.
    #[serde(default)]
    pub cross_plane_auth_dev_bypass: bool,
    #[serde(default)]
    pub browserbase_api_key: Option<String>,
    #[serde(default)]
    pub browserbase_project_id: Option<String>,
    #[serde(default)]
    pub browserbase_url: Option<String>,
    /// SERP provider — `brave`, `serper`, `searxng`, or empty/None to disable.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub search_provider: Option<String>,
    #[serde(default)]
    pub brave_search_key: Option<String>,
    #[serde(default)]
    pub serper_key: Option<String>,
    #[serde(default)]
    pub searxng_url: Option<String>,
    /// Stract self-hosted independent SERP base URL. Cycle 19 / cluster #17.
    /// When set, slots into the SearchProvider chain between Tantivy and
    /// SearXNG: `Tantivy → Stract → SearXNG → Brave`.
    #[serde(default)]
    pub stract_url: Option<String>,
    /// On-disk directory for the Tantivy local index. Cycle 19 / cluster #16.
    /// When unset, the index is in-memory (lost on restart). Production
    /// should point this at a persistent volume.
    #[serde(default)]
    pub local_index_dir: Option<String>,
    /// Model Plane gateway URL. With `QUARRY_EDGE__MODEL_AUDIO` on (QB-T17), /v1/audio routes
    /// to /v1/ai/speech + /v1/ai/transcribe via this URL.
    #[serde(default)]
    pub model_plane_url: Option<String>,
    /// Development-only static Model Plane bearer. Production uses Auth Core.
    #[serde(default)]
    pub model_plane_token: Option<String>,
    /// Routed Model Plane alias used by the page pipeline to name pages whose
    /// HTML `<title>` is missing or generic (`page_extracted.title_source =
    /// "model"`). Must be a routing alias the Model Plane intent layer
    /// resolves (`verevon-budget` = cheapest capable model, `verevon-balance`,
    /// `verevon-genius`), never a vendor model id. Default `verevon-budget`.
    /// Env: `QUARRY_EDGE__PAGE_TITLE_MODEL`.
    #[serde(default)]
    pub page_title_model: Option<String>,
    /// Page-title enrichment through the Model Plane (`QUARRY_EDGE__PAGE_TITLE_ENRICH`).
    /// Off by default: the hop needs a model path that the registry grants, and none exists
    /// today (see `model_plane_gate`). `true` takes effect only with that path; without it
    /// boot logs an error and extraction stays local (`title_source` is `html` or `host`).
    #[serde(default)]
    pub page_title_enrich: bool,
    /// BrowserBroker grant validation endpoint. When set, every agent run
    /// must present a grant_id and every action revalidates that grant.
    #[serde(default)]
    pub browser_grant_validator_url: Option<String>,
    /// BrowserBroker gRPC endpoint. This is the preferred production path
    /// when the `grpc` feature is enabled; the HTTP validator URL remains a
    /// compatibility shim for deployments that have not exposed gRPC yet.
    #[serde(default)]
    pub browser_grant_validator_grpc_url: Option<String>,
    /// Fail closed when a caller omits BrowserBroker's per-run grant.
    #[serde(default)]
    pub require_browser_grants: bool,
    /// Enable LLM-backed intent classification on the search router.
    /// When `true` AND `model_plane_url` is set, the router consults the
    /// Model Plane for query intent (Research / Comparative / Local /
    /// Code) on queries that don't match a rule fast-path. Default off
    /// to preserve deterministic latency until validated in production.
    #[serde(default)]
    pub llm_classify_intent: bool,
    /// Optional Model Plane model override for intent classification.
    /// Prefer a small fast model (e.g. Haiku). Empty/None → MP picks.
    #[serde(default)]
    pub llm_classify_model: Option<String>,
    /// Autoprompt — when `true` AND `model_plane_url` is set, the router
    /// rewrites verbose `Research`/`Comparative` queries into a tighter
    /// web-search query before fan-out. Degrade-safe (falls back to the
    /// original query). Default off, and it also needs a model path that the
    /// registry grants (see `model_plane_gate`).
    #[serde(default)]
    pub autoprompt: bool,
    /// Optional Model Plane model override for autoprompt rewriting.
    #[serde(default)]
    pub autoprompt_model: Option<String>,
    /// Answer synthesis (`POST /v1/answer`, and `include_answer` on search). Off by default.
    /// Needs a SearchProvider and a model path (see `model_plane_gate`).
    /// Env: `QUARRY_EDGE__ANSWER_SYNTHESIS`.
    #[serde(default)]
    pub answer_synthesis: bool,
    /// Structured extraction: `schema` on `POST /v1/extract`, the ai-formats `json` output.
    /// Off by default. Needs a model path (see `model_plane_gate`).
    /// Env: `QUARRY_EDGE__AI_FORMATS`.
    #[serde(default)]
    pub ai_formats: bool,
    /// Search suggestions (`POST /v1/search/suggest`: spelling, related queries, entity panel).
    /// Off by default. Needs a model path (see `model_plane_gate`).
    /// Env: `QUARRY_EDGE__SEARCH_SUGGEST`.
    #[serde(default)]
    pub search_suggest: bool,
    /// Audio (`POST /v1/audio`: speech and transcription through the Model Plane). Off by default.
    /// Needs the `model-audio` grant (see `model_plane_gate`).
    /// Env: `QUARRY_EDGE__MODEL_AUDIO`.
    #[serde(default)]
    pub model_audio: bool,
    /// Semantic rerank (Quarry option B). When `true` AND `data_plane_url` and the
    /// cross-plane tokens are set, the Data Plane's `POST /v1/rerank` orders the top-N
    /// merged results by a cross-encoder score. No Model Plane hop is made. Degrade-safe:
    /// any failure keeps the original order. Default off.
    #[serde(default)]
    pub semantic_rerank: bool,
    /// How many top results to rerank (the long tail is left in place). Default 10
    /// when unset, and at most 100 (the Data Plane's limit).
    #[serde(default)]
    pub semantic_rerank_top_n: Option<usize>,
    /// Shadow mode for the rerank (`QUARRY_EDGE__RERANK_SHADOW`). The Data Plane is asked
    /// and its answer measured, but the results keep their input order and carry no score.
    /// Used for the first deploy's smoke run. Default off. It has no effect unless
    /// `semantic_rerank` is on.
    #[serde(default)]
    pub rerank_shadow: bool,
    /// The budget of one Data Plane rerank call, in milliseconds (`QUARRY_EDGE__RERANK_DEADLINE_MS`).
    /// It is sent as the call's `deadline_ms`, and Quarry stops waiting at it plus a 0.5 s transport
    /// allowance. Unset, it is the runtime default (2 s). See [`EdgeConfig::rerank_deadline`].
    #[serde(default)]
    pub rerank_deadline_ms: Option<u64>,
    /// Zero-SaaS search posture (data residency): when `true`, the SmartRouter
    /// refuses to register external paid SERP providers (Brave/Serper) that send
    /// the query to a third party. Only in-infra providers (Tantivy / Stract /
    /// SearXNG + Data Plane) are used, so search queries never leave the cluster.
    /// Default `false` (Brave/Serper are registered as paid backups when keyed).
    #[serde(default)]
    pub zero_saas_search: bool,
    /// P1 / cluster #nats — NATS JetStream URL for cross-plane event
    /// fan-out (e.g. `nats://nats:4222`). When set, every `EventSink::emit`
    /// also publishes to JetStream so autocomplete-core, model-plane,
    /// and org-core can consume Quarry events directly. Empty/None
    /// leaves the sink mpsc-only (control-plane HTTP path unchanged).
    #[serde(default)]
    pub nats_url: Option<String>,
    /// Optional NATS auth token (NKEY/JWT). Mutually exclusive with
    /// `nats_creds_file`; if both are set, the credentials file wins.
    #[serde(default)]
    pub nats_token: Option<String>,
    /// Path to a `.creds` file (NATS user JWT + nkey seed). Production
    /// rollouts mount this from a secrets manager.
    #[serde(default)]
    pub nats_creds_file: Option<String>,
    /// Subject prefix on the JetStream stream. Default `quarry` matches
    /// the prefix that auto-create the `QUARRY_EVENTS` stream uses.
    #[serde(default)]
    pub nats_subject_prefix: Option<String>,
    /// P2 / cluster #grpc — Data Plane ingest transport. `"http"`
    /// (default) uses `IngestClient` (HTTP/JSON). `"grpc"` switches to
    /// `GrpcIngestAdapter` (HTTP/2 + protobuf). The `grpc` value is
    /// only meaningful when the binary was compiled with
    /// `--features grpc`; otherwise it falls back to HTTP with a
    /// warning. Wire empty/None to keep HTTP.
    #[serde(default)]
    pub data_plane_transport: Option<String>,
    /// D2 / cluster #14 — shared HMAC secret used to sign edge →
    /// control-plane requests. Hex-encoded; ≥ 32 chars (256 bits).
    /// When empty, requests go out unsigned (private-network dev
    /// posture only). Production MUST set this to the same value as
    /// `QUARRY_INTERNAL_SECRET` on the control plane.
    #[serde(default)]
    pub internal_secret: Option<String>,
    /// C30.1 / cluster #6 — Profile store backend.
    /// `"memory"` (default) — `InMemoryProfileStore` (dev / tests).
    /// `"postgres"` — `PostgresProfileStore` (requires `database_url`
    /// AND binary built with `--features postgres-queue`). When the
    /// feature isn't compiled in, this falls back to memory with a
    /// warning.
    #[serde(default)]
    pub profile_store_kind: Option<String>,
    /// C30.1 / cluster #1 + #6 — Postgres DSN used by the runtime
    /// for the durable request queue, profile store, and event
    /// history. Shared across all three because they live in the
    /// same database in production.
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue`; wired in follow-up
    pub database_url: Option<String>,
    /// C30.1 / cluster #7 — Enable durable job-history emit + the
    /// `GET /v1/runs/:id/events` read endpoint. Requires
    /// `database_url` AND `--features postgres-queue`.
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue`; wired in follow-up
    pub durable_event_history: bool,
    /// Phase-2 visual RAG producer — when all three are set (and `browser-agent`
    /// is on), ingested web pages are rendered to PNGs, written to the `cas_bucket`
    /// MinIO bucket, and `page_images.created` is published to `dataplane_nats_url`.
    /// `edge_internal_base_url` is the base of the image-serve endpoint the
    /// embedding-engine GETs. All unset (default) ⇒ producer inert.
    #[serde(default)]
    pub cas_bucket: Option<String>,
    #[serde(default)]
    pub dataplane_nats_url: Option<String>,
    #[serde(default)]
    pub edge_internal_base_url: Option<String>,
    /// Deterministic visual evidence processing. This is intentionally separate
    /// from page-image rendering: page images feed Data Plane embeddings, while
    /// visual observation processing produces debug/change artifacts for browser
    /// actions. Defaults off; the first backend is an isolated OpenCV sidecar.
    #[serde(default)]
    pub vision_enabled: bool,
    #[serde(default)]
    pub vision_backend: Option<String>,
    #[serde(default)]
    pub vision_sidecar_url: Option<String>,
    #[serde(default)]
    pub visual_diff_enabled: bool,
    #[serde(default)]
    pub screenshot_preprocessing_enabled: bool,
    #[serde(default)]
    pub page_image_cleanup_enabled: bool,
    #[serde(default)]
    pub visual_tiles_enabled: bool,
    #[serde(default)]
    pub ocr_preconditioning_enabled: bool,
    #[serde(default)]
    pub rendered_branding_visual_enabled: bool,
    #[serde(default)]
    pub visual_max_regions: Option<usize>,
    /// WS4.H: refuse durable resources (schedules, change monitors and snapshots,
    /// browser profile writes) for a Zero Data Retention organization. Off by
    /// default: a ZDR caller is served as before and the edge only logs a
    /// content-free `zdr.would_refuse` event per request (observe mode). Turn it
    /// on only after ADR-4 is signed off and customers are told.
    /// Env: `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`.
    #[serde(default)]
    pub zdr_refuse_durable_resources: bool,
    /// WS4.J: send a metadata-only tombstone to quarry-control when a ZDR run ends.
    /// OFF by default and not set by any env file, compose file or default: counsel
    /// review gates turning it on (it retains the fact that a ZDR run happened).
    /// Needs `control_base_url` and `internal_secret`; without them nothing is sent.
    /// Env: `QUARRY_EDGE__ZDR_TOMBSTONE`.
    #[serde(default)]
    pub zdr_tombstone: bool,
    /// WS17.P2: owner and ACL check on the `/v1/profiles*` routes. `observe` (the
    /// default) logs `profile.owner.would_deny` and serves as before; `enforce` answers
    /// 404 to a non-owner; `off` skips the check. Env: `QUARRY_EDGE__PROFILE_OWNER_ENFORCE`.
    #[serde(default)]
    pub profile_owner_enforce: crate::profile_guard::OwnerEnforce,
    /// F-t2-068: `POST /v1/profiles/:id/clear-data` (remove a stored profile's cookies, web storage
    /// and IndexedDB) answers 501 unless this is on. Off by default. Env: `QUARRY_EDGE__PROFILE_CLEAR_DATA`.
    #[serde(default)]
    pub profile_clear_data: bool,
    /// WS17.E1: envelope encryption of profile snapshots: `off` (default), `compat` or `on`.
    /// Validated at boot (`profile_encryption.rs`). Env: `QUARRY_EDGE__PROFILE_ENCRYPTION`.
    #[serde(default)]
    pub profile_encryption: String,
    /// WS17.E1: boot refuses any mode below `on` (set in the production overlay after the
    /// backfill). Env: `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED`.
    #[serde(default)]
    pub profile_encryption_required: bool,
    /// WS17.E1: key provider. Only `env` is built; `org_core` is WS17.E2. Env: `QUARRY_EDGE__PROFILE_KEY_PROVIDER`.
    #[serde(default)]
    pub profile_key_provider: String,
}

fn default_vector_retrieval_budget() -> u32 {
    quarry_runtime::vector_index::DEFAULT_PER_ORG_HOURLY_BUDGET
}

fn default_port() -> u16 {
    8080
}
fn default_ua() -> String {
    "Quarry/2.0 (+https://triodelab.com/quarry)".into()
}
fn default_timeout() -> u64 {
    30
}
fn default_control_url() -> String {
    "http://quarry-control:8081".into()
}
fn default_auth_core_url() -> String {
    "http://auth-core:3011".into()
}
fn default_artifact_backend() -> String {
    "memory".into()
}

/// The artifact backend name, normalised once for BOTH the production
/// durability gate and the store selection, so the two can never disagree about
/// what `S3` or ` Fs ` means.
pub fn normalize_artifact_backend(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}
fn default_artifact_root() -> String {
    "./data/artifacts".into()
}
fn default_cache_ttl() -> u64 {
    crate::cache::DEFAULT_TTL_SECS
}

/// Compose sets `QUARRY_EDGE__MODEL_PLANE_URL` to an empty string when the operator has not
/// opted in to the Model Plane (`${MODEL_PLANE_URL:-}`). An empty URL means "not configured", so
/// it becomes `None`. Left as `Some("")`, the cross-plane boot gate in main.rs would demand
/// `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` from a stack that never asked for the Model Plane.
fn blank_model_plane_url_is_unset(url: &mut Option<String>) {
    if url.as_deref().is_some_and(|raw| raw.trim().is_empty()) {
        *url = None;
    }
}

impl EdgeConfig {
    /// The budget of one rerank call: `QUARRY_EDGE__RERANK_DEADLINE_MS`, or the runtime default when
    /// unset, clamped to 100 ms to 8 s. That is the range the Data Plane accepts for `deadline_ms`, so
    /// the budget Quarry waits for is the budget it sends. A value outside the range is clamped.
    pub fn rerank_deadline(&self) -> Duration {
        let requested = self
            .rerank_deadline_ms
            .map_or(DEFAULT_RERANK_DEADLINE, Duration::from_millis);
        requested.clamp(RERANK_DEADLINE_MIN, RERANK_DEADLINE_MAX)
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::Environment::with_prefix("QUARRY_EDGE").separator("__"))
            .build()?;
        let mut edge: Self = cfg.try_deserialize().unwrap_or_else(|err| {
            // zdr-log-ok: runs once while the process boots, before any request or ZDR span exists; an operator needs the field name in the error
            tracing::error!(error = %err, "EdgeConfig failed to deserialize from environment; falling back to all-defaults (check for a required field with no #[serde(default)])");
            Self::defaults()
        });
        blank_model_plane_url_is_unset(&mut edge.model_plane_url);
        if edge.redis_url.is_none() {
            edge.redis_url = edge.dragonfly_url.clone();
        }
        Ok(edge)
    }

    pub fn defaults() -> Self {
        Self {
            port: default_port(),
            user_agent: default_ua(),
            fetch_timeout_s: default_timeout(),
            control_base_url: default_control_url(),
            redis_url: None,
            dragonfly_url: None,
            artifact_backend: default_artifact_backend(),
            artifact_root: default_artifact_root(),
            s3_bucket: None,
            control_api_key: String::new(),
            cache_ttl_secs: default_cache_ttl(),
            security_snapshot_path: None,
            data_plane_url: None,
            data_plane_ingest_url: None,
            data_plane_service_token: None,
            vector_retrieval: false,
            vector_retrieval_per_org_hourly_budget: default_vector_retrieval_budget(),
            auth_core_url: default_auth_core_url(),
            quarry_service_api_key: None,
            cross_plane_auth_dev_bypass: false,
            browserbase_api_key: None,
            browserbase_project_id: None,
            browserbase_url: None,
            search_provider: None,
            brave_search_key: None,
            serper_key: None,
            searxng_url: None,
            stract_url: None,
            local_index_dir: None,
            model_plane_url: None,
            model_plane_token: None,
            page_title_model: None,
            page_title_enrich: false,
            browser_grant_validator_url: None,
            browser_grant_validator_grpc_url: None,
            require_browser_grants: false,
            llm_classify_intent: false,
            llm_classify_model: None,
            autoprompt: false,
            autoprompt_model: None,
            answer_synthesis: false,
            ai_formats: false,
            search_suggest: false,
            model_audio: false,
            semantic_rerank: false,
            rerank_shadow: false,
            rerank_deadline_ms: None,
            semantic_rerank_top_n: None,
            zero_saas_search: false,
            nats_url: None,
            nats_token: None,
            nats_creds_file: None,
            nats_subject_prefix: None,
            data_plane_transport: None,
            internal_secret: None,
            profile_store_kind: None,
            database_url: None,
            durable_event_history: false,
            cas_bucket: None,
            dataplane_nats_url: None,
            edge_internal_base_url: None,
            vision_enabled: false,
            vision_backend: None,
            vision_sidecar_url: None,
            visual_diff_enabled: false,
            screenshot_preprocessing_enabled: false,
            page_image_cleanup_enabled: false,
            visual_tiles_enabled: false,
            ocr_preconditioning_enabled: false,
            rendered_branding_visual_enabled: false,
            visual_max_regions: None,
            zdr_refuse_durable_resources: false,
            zdr_tombstone: false,
            profile_owner_enforce: crate::profile_guard::OwnerEnforce::Observe,
            profile_clear_data: false,
            profile_encryption: String::new(),
            profile_encryption_required: false,
            profile_key_provider: String::new(),
        }
    }
}

#[cfg(test)]
mod artifact_backend_tests {
    use super::normalize_artifact_backend;

    #[test]
    fn prod_artifact_backend_match_is_case_insensitive() {
        for (raw, want) in [
            ("s3", "s3"),
            ("S3", "s3"),
            (" s3 ", "s3"),
            ("Fs", "fs"),
            ("FILESYSTEM", "filesystem"),
            ("Memory", "memory"),
        ] {
            assert_eq!(normalize_artifact_backend(raw), want, "{raw:?}");
        }
    }
}

#[cfg(test)]
mod compose_switch_tests {
    use super::{blank_model_plane_url_is_unset, EdgeConfig};
    use std::collections::HashMap;

    /// Reads the edge config from env-style pairs through the same `config` crate source that
    /// `EdgeConfig::from_env` uses, so the values the compose passes ("0", "") are parsed the
    /// way the running binary parses them.
    fn config_from(pairs: &[(&str, &str)]) -> EdgeConfig {
        let source: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        config::Config::builder()
            .add_source(
                config::Environment::with_prefix("QUARRY_EDGE")
                    .separator("__")
                    .source(Some(source)),
            )
            .build()
            .expect("the config builder accepts the pairs")
            .try_deserialize()
            .expect("EdgeConfig deserializes the compose values")
    }

    #[test]
    fn compose_off_values_parse_as_off_and_an_empty_url_is_unset() {
        let mut cfg = config_from(&[
            ("QUARRY_EDGE__MODEL_PLANE_URL", ""),
            ("QUARRY_EDGE__AUTOPROMPT", "0"),
            ("QUARRY_EDGE__SEMANTIC_RERANK", "0"),
            ("QUARRY_EDGE__LLM_CLASSIFY_INTENT", "0"),
            ("QUARRY_EDGE__PAGE_TITLE_ENRICH", "0"),
        ]);
        assert!(!cfg.autoprompt, "autoprompt is off");
        assert!(!cfg.semantic_rerank, "semantic rerank is off");
        assert!(!cfg.llm_classify_intent, "intent classification is off");
        assert!(!cfg.page_title_enrich, "page-title enrichment is off");
        blank_model_plane_url_is_unset(&mut cfg.model_plane_url);
        assert_eq!(cfg.model_plane_url, None, "an empty MODEL_PLANE_URL is not configured");
    }

    #[test]
    fn a_model_plane_url_that_is_set_is_kept() {
        let mut cfg = config_from(&[("QUARRY_EDGE__MODEL_PLANE_URL", "http://model-gateway:8080")]);
        blank_model_plane_url_is_unset(&mut cfg.model_plane_url);
        assert_eq!(cfg.model_plane_url.as_deref(), Some("http://model-gateway:8080"));
    }

    #[test]
    fn a_whitespace_only_url_is_unset() {
        let mut url = Some("   ".to_string());
        blank_model_plane_url_is_unset(&mut url);
        assert_eq!(url, None);
    }

    #[test]
    fn an_absent_url_stays_absent() {
        let mut cfg = config_from(&[]);
        blank_model_plane_url_is_unset(&mut cfg.model_plane_url);
        assert_eq!(cfg.model_plane_url, None);
        assert!(!cfg.autoprompt);
    }
}

#[cfg(test)]
mod tests {
    use super::EdgeConfig;
    use std::time::Duration;

    #[test]
    fn the_vector_leg_is_off_until_its_kill_switch_is_set() {
        let cfg = EdgeConfig::defaults();
        assert!(!cfg.vector_retrieval);
        assert_eq!(
            cfg.vector_retrieval_per_org_hourly_budget,
            quarry_runtime::vector_index::DEFAULT_PER_ORG_HOURLY_BUDGET
        );
    }

    #[test]
    fn the_rerank_is_off_and_not_in_shadow_by_default() {
        let cfg = EdgeConfig::defaults();
        assert!(!cfg.semantic_rerank);
        assert!(!cfg.rerank_shadow);
        assert_eq!(cfg.semantic_rerank_top_n, None);
    }

    /// The budget is the runtime default (2 s) unless set, and a set value is clamped to the range the
    /// Data Plane accepts (100 ms to 8 s), so the wait and the `deadline_ms` sent are the same number.
    #[test]
    fn the_rerank_deadline_is_two_seconds_unless_set_and_is_clamped_to_the_data_plane_range() {
        let mut cfg = EdgeConfig::defaults();
        assert_eq!(cfg.rerank_deadline(), Duration::from_millis(2_000));
        cfg.rerank_deadline_ms = Some(750);
        assert_eq!(cfg.rerank_deadline(), Duration::from_millis(750));
        cfg.rerank_deadline_ms = Some(5);
        assert_eq!(cfg.rerank_deadline(), Duration::from_millis(100));
        cfg.rerank_deadline_ms = Some(60_000);
        assert_eq!(cfg.rerank_deadline(), Duration::from_millis(8_000));
    }

    #[test]
    #[serial_test::serial]
    fn the_rerank_deadline_reads_from_its_env_variable() {
        std::env::set_var("QUARRY_EDGE__RERANK_DEADLINE_MS", "1500");
        let parsed = EdgeConfig::from_env();
        std::env::remove_var("QUARRY_EDGE__RERANK_DEADLINE_MS");
        let cfg = parsed.expect("the environment reads");
        assert_eq!(cfg.rerank_deadline_ms, Some(1500));
        assert_eq!(cfg.rerank_deadline(), Duration::from_millis(1500));
    }

    /// The compose block passes "0" or "1" as each switch's env value (QB-T17). Those strings must
    /// read as the bools, or `from_env` falls back to all defaults and every switch silently stays off.
    #[test]
    #[serial_test::serial]
    fn the_compose_values_read_as_the_model_plane_switches() {
        let set = [
            ("QUARRY_EDGE__ANSWER_SYNTHESIS", "1"),
            ("QUARRY_EDGE__AI_FORMATS", "0"),
            ("QUARRY_EDGE__SEARCH_SUGGEST", "1"),
            ("QUARRY_EDGE__MODEL_AUDIO", "0"),
            ("QUARRY_EDGE__PAGE_TITLE_ENRICH", "0"),
            ("QUARRY_EDGE__AUTOPROMPT", "1"),
        ];
        for (name, value) in set {
            std::env::set_var(name, value);
        }
        let parsed = EdgeConfig::from_env();
        for (name, _) in set {
            std::env::remove_var(name);
        }
        let cfg = parsed.expect("the environment reads");
        assert!(cfg.answer_synthesis);
        assert!(!cfg.ai_formats);
        assert!(cfg.search_suggest);
        assert!(!cfg.model_audio);
        assert!(!cfg.page_title_enrich);
        assert!(cfg.autoprompt);
    }
}
