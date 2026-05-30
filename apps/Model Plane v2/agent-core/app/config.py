"""Configuration for agent-core v2."""

from __future__ import annotations

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    """Environment-driven settings with sensible Docker defaults."""

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    # --- Service identity ---
    service_name: str = "agent-core-v2"
    service_version: str = "2.0.0"
    environment: str = "development"
    debug: bool = False

    # --- HTTP / gRPC ---
    host: str = "0.0.0.0"
    port: int = 8002
    grpc_port: int = 50053

    # --- PostgreSQL (reasoning-postgres) ---
    postgres_host: str = "postgres"
    postgres_port: int = 5432
    postgres_db: str = "agent_core_v2_db"
    postgres_user: str = "reasoning_user"
    postgres_password: str = ""
    postgres_pool_min: int = 2
    postgres_pool_max: int = 10

    # --- Redis (reasoning-redis) ---
    redis_url: str = "redis://redis:6379/2"
    redis_max_connections: int = 50
    redis_socket_timeout: float = 5.0

    # --- NATS: velion-nats (cross-plane, JetStream) ---
    nats_url: str = "nats://velion-nats:4222"
    nats_token: str = ""

    # --- NATS: reasoning-nats (local Model Plane) ---
    nats_local_url: str = "nats://nats:4222"
    nats_local_token: str = ""

    # --- NATS: controlplane-nats (billing-core usage events) ---
    control_plane_nats_url: str = ""
    control_plane_nats_token: str = ""

    # --- v2 service URLs (replace v1 ai-core) ---
    capability_core_url: str = "http://capability-core:8004"
    cost_core_url: str = "http://cost-core-v2:8006"
    llm_worker_url: str = "http://llm-worker:8005"
    llm_timeout: int = 120
    tool_execution_api_key: str = ""

    # --- Data Plane URLs (replaces documents-worker) ---
    data_plane_retrieval_url: str = "http://retrieval-service:8004"
    data_plane_documents_url: str = "http://documents-service:8001"

    # --- Ingestion Plane (Quarry-v2) for full-fat web fetch ---
    # When set, the `web_fetch` and `extract_structured` tools route
    # through Quarry's /v1/scrape so they get the full crawl pipeline:
    # JS rendering, TLS fingerprint emulation, robots/SSRF guards,
    # charset detection, soft-404 filtering, JSON-LD extraction, etc.
    # Unset (empty) → tools fall back to a plain httpx fetch (current
    # behaviour). Production should always set this.
    quarry_edge_url: str = ""
    # Bearer token for the Quarry edge. Required when quarry_edge_url
    # is set; the edge rejects unauthenticated requests in prod.
    quarry_edge_token: str = ""
    # Timeout for Quarry calls in seconds. Slightly higher than the
    # default httpx timeout because Quarry may do JS rendering.
    quarry_timeout_seconds: float = 30.0

    # --- LLM provider keys ---
    openai_api_key: str | None = None
    anthropic_api_key: str | None = None
    google_api_key: str | None = None

    # --- Agent runtime ---
    planner_model: str = "gpt-4o-mini"
    planner_provider: str = "openai"
    planner_temperature: float = 0.0
    max_run_actions: int = 10
    worker_count: int = 2
    lease_ttl_seconds: int = 300

    # --- Skills filesystem ---
    # Optional path to a directory containing .md skill files.
    # Files named SKILL.md or <name>.skill.md are loaded automatically.
    skills_directory: str | None = None
    # Minimum relevance score (0.0‒1.0) for a skill to be injected.
    skill_match_threshold: float = 0.15

    # --- Internal auth ---
    internal_api_key: str = ""

    # --- Data Plane internal auth key (sent as x-internal-key to retrieval/documents services) ---
    data_plane_internal_key: str = ""

    # --- Control Plane auth-core (JWT validation) ---
    auth_core_url: str = ""

    # --- Control Plane org-core (RBAC, quotas, entitlements) ---
    org_core_url: str = ""

    # --- OpenTelemetry ---
    otel_exporter_endpoint: str = ""

    # --- Rate limits (Phase C1) ---
    rate_limit_max_retries: int = 5

    # --- Prompt cache (Phase C2) ---
    disable_prompt_caching: bool = False

    # --- LSP integration (Phase C4) ---
    lsp_enabled: bool = False
    lsp_root_uri: str = ""

    # --- Voice I/O (Phase C5) ---
    voice_stt_backend: str = "stub"
    voice_tts_backend: str = "stub"

    # --- Plugin marketplace (Phase C6) ---
    plugin_catalog_url: str = ""
    plugin_managed_names: str = ""  # comma-separated

    # --- Coordinator mode (Phase D1) ---
    coordinator_enabled: bool = True

    # --- PydanticAI orchestration (Phase 1.2) ---
    pydantic_ai_enabled: bool = False  # flip to True once validated

    # --- Temporal (Phase 1 — durable workflows) ---
    temporal_host: str = "temporal-server:7233"
    temporal_namespace: str = "velion"
    temporal_task_queue: str = "agent-core"
    temporal_enabled: bool = True

    # --- Letta (Phase 5 — long-term memory) ---
    letta_api_url: str = "http://letta-server:8283"
    letta_enabled: bool = False

    # --- Object storage / RL export (Phase D) ---
    object_storage_endpoint: str = ""
    object_storage_access_key: str = ""
    object_storage_secret_key: str = ""

    # --- Self-improvement (Phase A–E) ---
    # Record trajectories for every completed/failed run
    trajectory_recording_enabled: bool = True
    # Publish org insights via NATS every N minutes (0 = disabled)
    org_insights_interval_minutes: int = 30
    # Atropos RL export: run nightly cron (disabled if object_storage_endpoint is empty)
    atropos_export_enabled: bool = True


    @property
    def dsn(self) -> str:
        return (
            f"postgresql://{self.postgres_user}:{self.postgres_password}"
            f"@{self.postgres_host}:{self.postgres_port}/{self.postgres_db}"
        )


settings = Settings()
